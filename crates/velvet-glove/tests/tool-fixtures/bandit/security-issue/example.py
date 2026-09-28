import subprocess


def run(user_input):
    subprocess.call("ls " + user_input, shell=True)
