# libp2p-node



## Getting started

The aim of this project is to create a p2p network using the libp2p library in rust.

## Installation
To install rust, follow this [link](https://doc.rust-lang.org/book/ch01-01-installation.html) and choose the system you want to install it on. 

## Start the project

Once you've installed rust, you'll need to follow these steps to launch the project:

1. Go to the project folder
```
cd project
```
2. Create and active a python virtual environment
```
python -m venv .env
source .env/bin/activate
```
2. Install maturin for building and publishing Rust-based Python packages with minimal configuration
```
pip install maturin
pip install patchelf
```
3. Build and execute the module
```
maturin develop

RUST_BACKTRACE=full python py_scripts/test.py  

```
<!-- 2. Install dependencies
```
cargo install --path .
```
3. Start the project: execute the command on many terminal.
```
cargo run
```-->
<!--Expected : 
```
Peer ID : 12D3KooWQafztgWfmHT4YbEYYG5ayTqFYdyaS12YPseuKgXTwS39.
Enter the command : 
ls p : list peers discovered.
ls l : local node listenning.
connected {peerID} : say if the local node is connected to this peerID.
connect {peerID} : connect local node to this peerID.
disconnect {peerID} : disconnect local node to this peerID.
connected list : list the connected node.
send {message} : send a message to all peers connected to this node.
stop : stop the process.
```