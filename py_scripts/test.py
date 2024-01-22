import pyo3_example
import time
from threading import Thread
import asyncio
from ctypes import cdll
from sys import platform
import sys

class Node(Thread):
    def __init__(self, address: str, port: int, topic: str, tcp: bool =True, udp: bool =False):
        super().__init__()
        self.daemon = True

        self.network = pyo3_example.Network(address, port, tcp, udp, topic)

        hash_set = set()
        hash_set.add(topic)

        self.topics = hash_set
    
    def send_message(self, topic: str, message: str):
        self.network.send_message(topic, message)
    
    def get_listening_address(self):
        res = self.network.get_listening_address()
        
        return res
    
    def get_discovered_peers(self) -> set:
        res = self.network.get_discovered_peers()

        return res
    
    def get_connected_peers(self) -> set:
        res = self.network.get_connected_peers()

        return res
    
    def subscribe_topic(self, topic: str):
        self.network.subscribe_topic(topic)
        self.topics.add(topic)

    def run(self) -> None:
        print("started")
        self.network.run()


if __name__ == '__main__':
    print("Create Node")

    node = Node(address="0.0.0.0", port=0, topic="test-net", tcp=True, udp=True)

    print("start node")
    node.start()

    time.sleep(1)

    a = 1

    while True:

        discov = node.get_discovered_peers()
        print(discov)

        time.sleep(5)