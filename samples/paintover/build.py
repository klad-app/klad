import json
import subprocess
import sys

klad, walk = sys.argv[1], sys.argv[2]


def run(*args):
    return subprocess.run([klad, *args], check=True, capture_output=True, text=True).stdout


run("add", walk, "layer", "paint", "name=brushwork", "id=brushwork")
timeline = json.loads(run("frames", walk, "--json"))
first = next(l for l in timeline["layers"] if not l["cels"][0]["id"].startswith("lines"))["cels"][0]["id"]
run("paintover", walk, "brushwork", "--frame", "0", "--sizes", "14,7", "--seed", "4")
last = timeline["frames"] - 1
print(run("paintover", walk, "brushwork", "--like", first, "--frames", f"0-{last}", "--sizes", "14,7", "--seed", "4"))
