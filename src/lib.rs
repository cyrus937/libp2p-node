use futures::channel::{mpsc, oneshot};
use futures::{prelude::*, SinkExt};
use libp2p::{
    futures::StreamExt,
    gossipsub::{self, MessageId, TopicHash},
    mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId, Swarm,
};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    error::Error,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};
use tokio::io;
use tokio::runtime::Handle;
use tracing_subscriber::EnvFilter;

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

#[derive(Clone)]
#[pyclass(name = "Node", subclass)]
pub struct Node {
    pub id: PeerId,
    pub message: String,
    #[pyo3(get, set)]
    pub connected_peers: HashSet<String>,
    #[pyo3(get, set)]
    pub discovered_peers: HashSet<String>,
    #[pyo3(get, set)]
    pub listening_address: HashSet<String>,
    pub swarm: Arc<tokio::sync::Mutex<Swarm<MyBehaviour>>>,
    command_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<Command>>>,
    event_sender: mpsc::Sender<Event>,
}

#[derive(Clone)]
#[pyclass(name = "Client", subclass)]
pub struct Client {
    id: PeerId,
    sender: mpsc::Sender<Command>,
    topics: HashMap<String, TopicHash>,
}

#[derive(Clone)]
#[pyclass(name = "Network", subclass)]
pub struct Network {
    #[pyo3(get, set)]
    pub client: Client,
    #[pyo3(get, set)]
    pub node: Node,
    event_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<Event>>>,
}

#[derive(Clone)]
pub struct Test<'a> {
    pub node: &'a Node,
}

#[pymodule]
fn pyo3_example(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Node>()?;
    m.add_class::<Client>()?;
    m.add_class::<Network>()?;
    Ok(())
}

#[pymethods]
impl Network {
    #[new]
    pub fn new<'a>(
        address: Option<&'a str>,
        port: Option<u32>,
        tcp: Option<bool>,
        udp: Option<bool>,
        msg_topic: Option<&'a str>,
    ) -> PyResult<Network> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let t = rt.block_on(async {
            create_node(address, port, tcp, udp, msg_topic)
                .await
                .unwrap()
        });
        let node = t.2;

        Ok(Network {
            client: t.0,
            node,
            event_receiver: t.1,
        })
    }

    fn run<'py>(&self, py: Python<'py>) {
        let network = self.clone();
        let handle = tokio::runtime::Runtime::new().unwrap();
        let _ = handle.block_on(async {
            let mut node = network.node;
            let swarm_arc_clone = Arc::clone(&node.swarm);
            let mut swarm_guard = swarm_arc_clone.lock().await;
            let swarm = &mut *swarm_guard;

            let command_arc_clone = Arc::clone(&node.command_receiver);
            let mut command_guard = command_arc_clone.lock().await;
            let command_receiver = &mut *command_guard;

            loop {
                // let evt = {
                futures::select! {
                    event = swarm.next() => handle_event(&mut node, event.expect("Swarm stream to be infinite."), swarm).await,
                    command = command_receiver.next() => match command {
                        Some(c) => {
                            handle_command(&mut node, c, swarm).await},
                        // Command channel closed, thus shutting down the network event loop.
                        None=>  {},
                    },
                };
            }
        }
        );
        println!("Out of the runtime");
    }

    fn subscribe_topic(&mut self, topic: String) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        let _ = rt.block_on(async { self.client.async_subscribe_topic(topic).await });
    }

    fn unsubscribe_topic(&mut self, topic: String) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        let _ = rt.block_on(async { self.client.async_unsubscribe_topic(topic).await });
    }

    pub fn get_connected_peers(&mut self) -> HashSet<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        rt.block_on(async { self.client.async_get_connected_peers().await })
    }

    pub fn get_discovered_peers(&mut self) -> HashSet<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        rt.block_on(async { self.client.async_get_discovered_peers().await })
    }

    pub fn get_listening_address(&mut self) -> HashSet<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        rt.block_on(async { self.client.async_get_listening_address().await })
    }

    fn send_message(&mut self, topic: String, msg: String) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // let rt = Handle::current();
        let _ = rt.block_on(async { self.client.async_send_message(topic, msg).await });
    }
}

impl Client {
    /// Subscribe to a new topic
    async fn async_subscribe_topic(&mut self, topic: String) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::SubscribeTopic { topic, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Unsubscribe to a topic
    async fn async_unsubscribe_topic(
        &mut self,
        topic: String,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::UnSubscribeTopic { topic, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Get the list of connected peers
    async fn async_get_connected_peers(&mut self) -> HashSet<String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::GetConnectedPeers { sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Get the list of discovered peers
    async fn async_get_discovered_peers(&mut self) -> HashSet<String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::GetDiscoveredPeers { sender })
            .await
            .expect("Command receiver not to be dropped.");
        let res = receiver.await.expect("Sender not to be dropped.");

        for element in &res {
            let peer_id = peer_id_from_str(element.as_str());
            self.sender
                .send(Command::ConnectPeer { peer_id })
                .await
                .expect("Error");
        }

        res
    }

    /// Get listening address
    async fn async_get_listening_address(&mut self) -> HashSet<String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::ListeningAddress { sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Get listening address
    async fn async_send_message(&mut self, topic: String, message: String) {
        let topic_hash = self.topics.get(&topic);
        match topic_hash {
            Some(val) => self
                .sender
                .send(Command::SendToOne {
                    topic: val.clone(),
                    message,
                })
                .await
                .expect("Command receiver not to be dropped"),
            None => {
                println!("not found")
            }
        }
    }
}

async fn create_node(
    address: Option<&str>,
    port: Option<u32>,
    tcp: Option<bool>,
    udp: Option<bool>,
    msg_topic: Option<&str>,
) -> Result<(Client, Arc<tokio::sync::Mutex<mpsc::Receiver<Event>>>, Node), Box<dyn Error>> {
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

    let (command_sender, command_receiver) = mpsc::channel(10);
    let (event_sender, event_receiver) = mpsc::channel(1);

    let mut topics = HashMap::new();
    topics.insert(msg_topic.unwrap().to_string(), topic.into());

    let client = Client {
        id: *swarm.local_peer_id(),
        sender: command_sender,
        topics,
    };

    let id = *swarm.local_peer_id();

    let node = Node {
        id,
        message: String::new(),
        connected_peers: HashSet::new(),
        swarm: Arc::new(tokio::sync::Mutex::new(swarm)),
        command_receiver: Arc::new(tokio::sync::Mutex::new(command_receiver)),
        event_sender,
        discovered_peers: HashSet::new(),
        listening_address: HashSet::new(),
    };

    Ok((
        client,
        Arc::new(tokio::sync::Mutex::new(event_receiver)),
        node,
    ))
}

async fn handle_event(
    node: &mut Node,
    event: SwarmEvent<MyBehaviourEvent>,
    swarm: &mut Swarm<MyBehaviour>,
) {
    match event {
        SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            propagation_source: peer_id,
            message_id: id,
            message,
        })) => {
            println!(
                "Got message: '{}' with id: {id} from peer: {peer_id}",
                String::from_utf8_lossy(&message.data),
            );
        }
        SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
            for (peer_id, _multiaddr) in list {
                println!("mDNS discover peer has discovered: {}", peer_id);
                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
            }
        }
        SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
            for (peer_id, _multiaddr) in list {
                println!("mDNS discover peer has expired: {}", peer_id);
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);
            }
        }
        SwarmEvent::ConnectionEstablished {
            peer_id,
            connection_id,
            endpoint,
            num_established,
            concurrent_dial_errors,
            established_in,
        } => {
            println!("Connection Established: {}", peer_id);
        }
        SwarmEvent::ConnectionClosed {
            peer_id,
            connection_id,
            endpoint,
            num_established,
            cause,
        } => {
            println!("{}", cause.unwrap().to_string());
        }
        SwarmEvent::IncomingConnection {
            connection_id,
            local_addr,
            send_back_addr,
        } => {
            println!("In comming connection: {}", connection_id);
        }
        SwarmEvent::IncomingConnectionError {
            connection_id,
            local_addr,
            send_back_addr,
            error,
        } => {
            println!(
                "In comming connection error: {} error {}",
                connection_id, error
            );
        }
        SwarmEvent::OutgoingConnectionError {
            connection_id,
            peer_id,
            error,
        } => {
            println!(
                "Out going connection error: {} error {}",
                connection_id, error
            );
        }
        SwarmEvent::NewListenAddr {
            listener_id,
            address,
        } => {
            println!("new listen address: {}", address);
        }
        SwarmEvent::ExpiredListenAddr {
            listener_id,
            address,
        } => {
            println!("expired listen address: {}", address);
        }
        SwarmEvent::ListenerClosed {
            listener_id,
            addresses,
            reason,
        } => {
            println!("Listener closed: {} ", listener_id);
            addresses.iter().for_each(|f| println!("{}", f))
        }
        SwarmEvent::ListenerError { listener_id, error } => {
            println!("Listener error: {} error {}", listener_id, error);
        }
        SwarmEvent::Dialing {
            peer_id,
            connection_id,
        } => {
            println!(
                "Dialing peer: {} with connection: {}",
                peer_id.unwrap(),
                connection_id
            );
        }
        SwarmEvent::NewExternalAddrCandidate { address } => {
            println!("new external address candidate: {}", address);
        }
        SwarmEvent::ExternalAddrConfirmed { address } => {
            println!("external address confirmed: {}", address);
        }
        SwarmEvent::ExternalAddrExpired { address } => {
            println!("external address expired: {}", address);
        }
        _ => {}
    }
}

async fn handle_command(node: &mut Node, command: Command, swarm: &mut Swarm<MyBehaviour>) {
    match command {
        Command::SubscribeTopic { topic, sender } => {
            let topic = gossipsub::IdentTopic::new(topic.as_str());
            // let mut swarm = node.swarm.lock().await;
            swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();
            // node.topics.insert(topic.into());
        }
        Command::UnSubscribeTopic { topic, sender } => {
            let topic = gossipsub::IdentTopic::new(topic.as_str());
            // let mut swarm = node.swarm.lock().await;
            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic).unwrap();
            // node.topics.remove(&topic.into());
        }
        Command::GetConnectedPeers { sender } => {
            let mut list = HashSet::new();
            swarm.behaviour().gossipsub.all_peers().for_each(|f| {
                list.insert((*f.0).to_string());
            });
            sender.send(list).unwrap();
        }
        Command::GetDiscoveredPeers { sender } => {
            let mut list = HashSet::new();
            let nodes = swarm.behaviour().mdns.discovered_nodes();
            for peer in nodes {
                list.insert((*peer).to_string());
            }
            sender.send(list).unwrap();
        }
        Command::ListeningAddress { sender } => {
            let mut list = HashSet::new();
            swarm.listeners().for_each(|f| {
                list.insert(f.to_string());
            });
            sender.send(list).unwrap();
        }
        Command::SendToOne { topic, message } => {
            println!("Message : {}", message);
            if let Err(e) = swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic, message.as_bytes())
            {
                println!("Publish error: {e:?}");
            }
        }
        Command::ConnectPeer { peer_id } => {
            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
        }
    }
}

#[derive(Debug)]
enum Command {
    SubscribeTopic {
        topic: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    UnSubscribeTopic {
        topic: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    SendToOne {
        topic: TopicHash,
        message: String,
    },
    GetConnectedPeers {
        sender: oneshot::Sender<HashSet<String>>,
    },
    GetDiscoveredPeers {
        sender: oneshot::Sender<HashSet<String>>,
    },
    ListeningAddress {
        sender: oneshot::Sender<HashSet<String>>,
    },
    ConnectPeer {
        peer_id: PeerId,
    },
}

enum Event {
    InCommingMessage {
        propagation_source: PeerId,
        message_id: MessageId,
        topic: TopicHash,
        message: String,
    },
}

fn peer_id_from_str(val: &str) -> PeerId {
    let bytes = bs58::decode(val).into_vec().unwrap();
    let peer_id = PeerId::from_bytes(&bytes).unwrap();
    peer_id
}
