import type { CoreEvent, Missing } from "./api";

export class Task {
  running = $state(false);
  stage = $state("");
  done = $state(0);
  total = $state(0);
  error = $state<string | null>(null);
  /** Launcher messages and game output, newest last (capped). */
  log = $state<string[]>([]);
  exitCode = $state<number | null>(null);
  playing = $state(false);
  /** Set when the task stopped because files must come through the browser. */
  needs = $state<Missing[] | null>(null);
  /** What to run again once they are there. */
  resume = $state<"play" | "update">("play");

  push(line: string) {
    this.log.push(line);
    if (this.log.length > 3000) this.log.splice(0, this.log.length - 3000);
  }
}

class Tasks {
  // A plain Map on purpose: get() runs during rendering, where writing to
  // $state is not allowed. Each Task's own fields are reactive.
  #map = new Map<string, Task>();

  get(id: string): Task {
    let t = this.#map.get(id);
    if (!t) {
      t = new Task();
      this.#map.set(id, t);
    }
    return t;
  }

  begin(id: string) {
    const t = this.get(id);
    t.running = true;
    t.error = null;
    t.exitCode = null;
    t.needs = null;
    t.stage = "Starting…";
    t.done = t.total = 0;
  }

  handle(id: string, e: CoreEvent) {
    const t = this.get(id);
    t.running = true;
    switch (e.kind) {
      case "stage":
        t.stage = e.text;
        t.done = t.total = 0;
        t.playing = e.text === "Running";
        t.push(`== ${e.text}`);
        break;
      case "progress":
        t.done = e.done;
        t.total = e.total;
        break;
      case "log":
        t.push(e.text);
        break;
      case "game":
        t.push(e.line);
        break;
    }
  }

  done(id: string, error?: string | null, exitCode?: number | null, needs?: Missing[] | null, kind?: "play" | "update") {
    const t = this.get(id);
    t.needs = needs?.length ? needs : null;
    if (kind) t.resume = kind;
    if (t.needs) t.push(`== ${t.needs.length} file(s) need a browser download`);
    t.running = false;
    t.playing = false;
    t.stage = "";
    t.error = error ?? null;
    t.exitCode = exitCode ?? null;
    if (error) t.push(`!! ${error}`);
    if (exitCode != null) t.push(`== Game closed (exit code ${exitCode})`);
  }
}

export const tasks = new Tasks();
