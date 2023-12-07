import sys
import node

def run_with_error():
    print("Create Node")
    nod = node.Node("0.0.0.0", 0, True, True, "test-net")
    print("Start Node")
    nod.run()

def run_without_error():
    node.create_run("0.0.0.0", 0, True, True, "test-net")

if __name__ == '__main__':
    
    if (sys.argv.__len__() > 1 and sys.argv[1] == "err") :
        run_with_error()
    else:
        run_without_error()
    