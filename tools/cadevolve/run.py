#!/usr/bin/env python3
"""Run the CadEvolve generators (CADEvolve-G/parametric_generators.json) through a `cadquery`.

    run.py --python <venv python> [--path <dir to put first on PYTHONPATH>] --out results.jsonl
           [--jobs N] [--timeout S] [--only name,name] [--match substring] [--limit N]

Each generator runs in a worker process with its default arguments; a worker that passes the
timeout is killed and replaced. Results are JSON lines (see worker.py), one per generator."""
import argparse, json, os, select, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.expanduser("~/Work/cadevolve/CADEvolve-G/parametric_generators.json")

class Worker:
    def __init__(self, args, env):
        self.p = subprocess.Popen([args.python, "-I" if not args.path else "-u", os.path.join(HERE, "worker.py"), str(args.mem << 30)],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  env=env, text=True, bufsize=1, cwd=args.cwd)
        self.task = None
        self.started = 0.0

    def give(self, task):
        self.task = task
        self.started = time.time()
        self.p.stdin.write(json.dumps(task) + "\n")
        self.p.stdin.flush()

    def kill(self):
        self.p.kill()
        self.p.wait()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python", required=True)
    ap.add_argument("--path", default="")
    ap.add_argument("--out", required=True)
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--timeout", type=float, default=60)
    ap.add_argument("--mem", type=int, default=3, help="GiB of address space per worker")
    ap.add_argument("--only", default="")
    ap.add_argument("--from-results", default="", help="rerun only the names in this results file")
    ap.add_argument("--match", default="")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--data", default=DATA)
    args = ap.parse_args()
    args.cwd = os.path.join(HERE, "cwd")
    os.makedirs(args.cwd, exist_ok=True)
    gens = json.load(open(args.data))
    tasks = [{"i": i, "name": g["name"], "code": g["code"]} for i, g in enumerate(gens)]
    if args.only:
        keep = set(args.only.split(","))
        tasks = [t for t in tasks if t["name"] in keep]
    if args.from_results:
        keep = {json.loads(l)["i"] for l in open(args.from_results)}
        tasks = [t for t in tasks if t["i"] in keep]
    if args.match:
        tasks = [t for t in tasks if args.match in t["name"]]
    if args.limit:
        tasks = tasks[: args.limit]
    env = dict(os.environ)
    if args.path:
        env["PYTHONPATH"] = args.path
    env["PYTHONUNBUFFERED"] = "1"
    todo = list(reversed(tasks))
    workers = [Worker(args, env) for _ in range(min(args.jobs, len(todo)))]
    done = 0
    t0 = time.time()
    with open(args.out, "w") as out:
        def finish(res):
            nonlocal done
            out.write(json.dumps(res) + "\n")
            out.flush()
            done += 1
            if done % 100 == 0 or done == len(tasks):
                print(f"{done}/{len(tasks)}  {time.time() - t0:.0f} s", file=sys.stderr, flush=True)
        for w in workers:
            if todo:
                w.give(todo.pop())
        while any(w.task for w in workers):
            busy = {w.p.stdout.fileno(): w for w in workers if w.task}
            ready, _, _ = select.select(list(busy), [], [], 0.5)
            for fd in ready:
                w = busy[fd]
                line = w.p.stdout.readline()
                if not line:  # the worker died (a crash, or out of memory)
                    w.kill()
                    finish({"i": w.task["i"], "name": w.task["name"], "status": "crash",
                            "secs": round(time.time() - w.started, 3)})
                    idx = workers.index(w)
                    workers[idx] = w = Worker(args, env)
                else:
                    finish(json.loads(line))
                    w.task = None
                if todo:
                    w.give(todo.pop())
                else:
                    w.task = None
            now = time.time()
            for idx, w in enumerate(workers):
                if w.task and now - w.started > args.timeout:
                    w.kill()
                    finish({"i": w.task["i"], "name": w.task["name"], "status": "timeout", "secs": args.timeout})
                    workers[idx] = nw = Worker(args, env)
                    if todo:
                        nw.give(todo.pop())
        for w in workers:
            w.p.stdin.close()
            w.kill()

if __name__ == "__main__":
    main()
