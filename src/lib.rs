use futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::Swarm;
use libp2p::{
    core::multiaddr::Multiaddr, gossipsub, mdns, noise, swarm::NetworkBehaviour, tcp, yamux, PeerId,
};
use pyo3::{prelude::*, wrap_pyfunction};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;
use tokio::{io, select};
use tracing_subscriber::EnvFilter;

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

#[pyclass(name = "Node", subclass)]
pub struct Node {
    pub id: PeerId,
    pub node_listening: HashSet<Multiaddr>,
    pub peers_id: HashSet<PeerId>,
    pub swarm: Swarm<MyBehaviour>,
    pub topic: gossipsub::IdentTopic,
}

#[pymodule]
fn node(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Node>()?;
    m.add_wrapped(wrap_pyfunction!(create_run))?;
    Ok(())
}
pub enum EventType {
    Response(String),
    Input(String),
}

#[pymethods]
impl Node {
    ///----------------------ERROR--------------------
    #[new]
    pub fn new(
        address: Option<&str>,
        port: Option<u32>,
        tcp: Option<bool>,
        udp: Option<bool>,
        msg_topic: Option<&str>,
    ) -> Self {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let node = rt.block_on(async {
            create_node(address, port, tcp, udp, msg_topic)
                .await
                .unwrap()
        });
        node
    }

    fn run(&mut self) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            async_run(self).await;
        });
        Ok(())
    }
    ///-------------------------------------------------
    
    #[getter]
    fn get_id(&self) -> PyResult<String> {
        Ok(self.id.to_string())
    }


    fn handle_list_peers(&mut self) -> PyResult<()> {
        println!("Discovered Peers:");
        let nodes = self.swarm.behaviour().mdns.discovered_nodes();
        let mut unique_peers = HashSet::new();
        for peer in nodes {
            unique_peers.insert(peer);
            self.peers_id.insert(*peer);
        }
        unique_peers.iter().for_each(|p| println!("{}", p));
        Ok(())
    }

    pub fn send_message(&mut self, cmd: &str) {
        if let Some(rest) = cmd.strip_prefix("send ") {
            if let Err(e) = self
                .swarm
                .behaviour_mut()
                .gossipsub
                .publish(self.topic.clone(), rest.as_bytes())
            {
                println!("Publish error: {e:?}");
            }
            // }
        }
    }

    pub fn handle_list_listening_node(&mut self) {
        println!("Local node is listenning on :");
        self.swarm.listeners().for_each(|f| println!("{}", f));
    }

    #[pyo3(signature = (cmd))]
    pub fn is_connected_to(&mut self, cmd: &str) {
        if let Some(val) = cmd.strip_prefix("connected ") {
            if val == "list" {
                println!("Connected Peers : ");
                self.swarm.connected_peers().for_each(|f| println!("{}", f));
            } else {
                let mut is_connected: bool = false;
                let nodes = self.swarm.behaviour().mdns.discovered_nodes();
                for peer in nodes {
                    if peer.to_string().as_str() == val {
                        is_connected = self.swarm.is_connected(&peer);
                        // println!("ICI");
                        break;
                    }
                }
                if is_connected {
                    println!("This node is connected to {}", val);
                } else {
                    println!("This node is not connected to {}", val);
                }
            }
        }
    }

    pub fn disconnect_to(&mut self, cmd: &str) {
        if let Some(val) = cmd.strip_prefix("disconnect ") {
            let peer_id = peer_id_from_str(val);

            let _ = self.swarm.disconnect_peer_id(peer_id);

            self.swarm
                .behaviour_mut()
                .gossipsub
                .remove_explicit_peer(&peer_id);
        }
    }

    pub fn connect_to(&mut self, cmd: &str) {
        if let Some(val) = cmd.strip_prefix("connect ") {
            let peer_id = peer_id_from_str(val);

            let _ = self
                .swarm
                .behaviour_mut()
                .gossipsub
                .add_explicit_peer(&peer_id);
        }
    }
}

fn peer_id_from_str(val: &str) -> PeerId {
    let bytes = bs58::decode(val).into_vec().unwrap();
    let peer_id = PeerId::from_bytes(&bytes).unwrap();
    peer_id
}


#[pyfunction]
fn create_run(address: Option<&str>,
    port: Option<u32>,
    tcp: Option<bool>,
    udp: Option<bool>,
    msg_topic: Option<&str>,
) -> PyResult<()> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async { async_create_run(address, port, tcp, udp, msg_topic).await });
    Ok(())
}



async fn create_node(
    address: Option<&str>,
    port: Option<u32>,
    tcp: Option<bool>,
    udp: Option<bool>,
    msg_topic: Option<&str>,
) -> Result<Node, Box<dyn Error>> {
    let addr: &str = match address {
        Some(val) => val,
        None => "0.0.0.0",
    };

    let po = match port {
        Some(val) => val,
        None => 0,
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default().port_reuse(true).nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_quic()
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            // Set a custom gossipsub configuration
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?; // Temporary hack because `build` does not return a proper `std::error::Error`.

            // build a gossipsub network behaviour
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;
            Ok(MyBehaviour { gossipsub, mdns })
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Create a Gossipsub topic
    let topic = gossipsub::IdentTopic::new(msg_topic.unwrap());
    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();

    // Listen on all interfaces and whatever port the OS assigns
    if tcp.unwrap() {
        let tcp_address = format!("/ip4/{}/tcp/{}", addr, po);
        swarm
            .listen_on(tcp_address.to_owned().parse().unwrap())
            .unwrap();
    }
    if udp.unwrap() {
        let udp_address = format!("/ip4/{}/udp/{}/quic-v1", addr, po);
        swarm
            .listen_on(udp_address.to_owned().parse().unwrap())
            .unwrap();
    } else if !tcp.unwrap() {
        let tcp_address = format!("/ip4/{}/tcp/{}", addr, po);
        swarm
            .listen_on(tcp_address.to_owned().parse().unwrap())
            .unwrap();
    }

    Ok(Node {
        id: *swarm.local_peer_id(),
        node_listening: HashSet::new(),
        peers_id: HashSet::new(),
        swarm: swarm,
        topic,
    })
}

async fn async_run(node: &mut Node) {

    println!("Peer ID : {}", node.id);
    println!("Enter the command : ");
    println!("ls p : list peers discovered.");
    println!("ls l : local node listenning.");
    println!("connected peerID : say if the local node is connected to this peerID.");
    println!("connect peerID : connect local node to this peerID.");
    println!("disconnect peerID : disconnect local node to this peerID.");
    println!("connected list : list the connected node.");
    println!("send message : send a message to all peers connected to this node.");
    println!("stop : stop the process.");

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        let evt = {
            tokio::select! {
                line = stdin.next_line() => Some(EventType::Input(line.expect("can get line").expect("can read line from stdin"))),
                event = node.swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                    propagation_source: peer_id,
                                    message_id: id,
                                    message,
                                })) => {println!(
                                        "Got message: '{}' with id: {id} from peer: {peer_id}",
                                        String::from_utf8_lossy(&message.data),
                                    );
                                None
                            },
                                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                                    for (peer_id, _multiaddr) in list {
                                        node.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    }; None
                                },
                                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                                    for (peer_id, _multiaddr) in list {
                                        println!("mDNS discover peer has expired: {peer_id}");
                                        node.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                    }; None
                                },
                    _ => {None}
                },
            }
        };

        if let Some(event) = evt {
            match event {
                EventType::Response(resp) => {
                
                    println!("{}", resp)
                }
                EventType::Input(line) => match line.as_str() {
                    "ls p" => async_handle_list_peers(node).await,
                    "ls l" => node.handle_list_listening_node(),
                    cmd if cmd.starts_with("send ") => node.send_message(cmd),
                    cmd if cmd.starts_with("connected ") => node.is_connected_to(cmd),
                    cmd if cmd.starts_with("connect ") => node.connect_to(cmd),
                    cmd if cmd.starts_with("disconnect ") => node.disconnect_to(cmd),
                    "stop" => break,
                    _ => println!("unknown command"),
                },
            }
        }
    }
}

async fn async_create_run(
    address: Option<&str>,
    port: Option<u32>,
    tcp: Option<bool>,
    udp: Option<bool>,
    msg_topic: Option<&str>,
) {

    let mut node: Node = create_node(
        address,
        port,
        tcp,
        udp,
        msg_topic,
    )
    .await
    .unwrap();

    println!("Peer ID : {}", node.id);
    println!("Enter the command : ");
    println!("ls p : list peers discovered.");
    println!("ls l : local node listenning.");
    println!("connected peerID : say if the local node is connected to this peerID.");
    println!("connect peerID : connect local node to this peerID.");
    println!("disconnect peerID : disconnect local node to this peerID.");
    println!("connected list : list the connected node.");
    println!("send message : send a message to all peers connected to this node.");
    println!("stop : stop the process.");

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        let evt = {
            tokio::select! {
                line = stdin.next_line() => Some(EventType::Input(line.expect("can get line").expect("can read line from stdin"))),
                event = node.swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                    propagation_source: peer_id,
                                    message_id: id,
                                    message,
                                })) => {println!(
                                        "Got message: '{}' with id: {id} from peer: {peer_id}",
                                        String::from_utf8_lossy(&message.data),
                                    );
                                None
                            },
                                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                                    for (peer_id, _multiaddr) in list {
                                        // println!("mDNS discovered a new peer: {peer_id}");
                                        node.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    }; None
                                },
                                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                                    for (peer_id, _multiaddr) in list {
                                        println!("mDNS discover peer has expired: {peer_id}");
                                        node.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                    }; None
                                },
                    _ => {None}
                },
            }
        };

        if let Some(event) = evt {
            match event {
                EventType::Response(resp) => {
                    println!("{}", resp)
                }
                EventType::Input(line) => match line.as_str() {
                    "ls p" => async_handle_list_peers(&mut node).await,
                    "ls l" => node.handle_list_listening_node(),
                    cmd if cmd.starts_with("send ") => node.send_message(cmd),
                    cmd if cmd.starts_with("connected ") => node.is_connected_to(cmd),
                    cmd if cmd.starts_with("connect ") => node.connect_to(cmd),
                    cmd if cmd.starts_with("disconnect ") => node.disconnect_to(cmd),
                    "stop" => break,
                    _ => println!("unknown command"),
                },
            }
        }
    }
}

async fn async_handle_list_peers(node: &mut Node) {
    println!("Discovered Peers:");
    let nodes = node.swarm.behaviour().mdns.discovered_nodes();
    let mut unique_peers = HashSet::new();
    for peer in nodes {
        unique_peers.insert(peer);
        node.peers_id.insert(*peer);
    }
    unique_peers.iter().for_each(|p| println!("{}", p));
}