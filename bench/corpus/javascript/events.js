"use strict";

const EventEmitter = require("events");
const { promisify } = require("util");

/** Channels, configuration reads, classes, closures and async iteration. */
const emitter = new EventEmitter();
const sleep = promisify(setTimeout);

class Queue extends EventEmitter {
  #items = [];
  static #instances = 0;

  constructor(limit = Number(process.env.QUEUE_LIMIT || 10)) {
    super();
    this.limit = limit;
    Queue.#instances += 1;
  }

  push(item) {
    if (this.#items.length >= this.limit) {
      this.emit("overflow", item);
      return false;
    }
    this.#items.push(item);
    this.emit("pushed", item);
    return true;
  }

  get size() {
    return this.#items.length;
  }

  async *drain(delay = 0) {
    while (this.#items.length) {
      await sleep(delay);
      yield this.#items.shift();
    }
  }
}

function counter(start = 0) {
  let n = start;
  return {
    next: () => ++n,
    reset() {
      n = start;
    },
  };
}

const pattern = /^(?<name>[a-z]+)-(?<id>\d{2,})$/iu;
const label = (s) => s.match(pattern)?.groups?.name ?? "unnamed";

emitter.on("job.created", (job) => console.log(`created ${job.id}`));
emitter.on("job.failed", async ({ id, error }) => {
  await sleep(1);
  console.error(`job ${id}: ${error?.message ?? "unknown"}`);
});

async function main() {
  const queue = new Queue();
  queue.on("pushed", (item) => emitter.emit("job.created", item));
  ["a-10", "b-22", "ç-33"].forEach((name, i) => queue.push({ id: i, name: label(name) }));
  for await (const item of queue.drain()) {
    if (!item.name) emitter.emit("job.failed", { id: item.id, error: new Error("no name") });
  }
  const { next } = counter(process.env.START ? parseInt(process.env.START, 10) : 0);
  return [next(), next(), `${process.env.HOME}/out`];
}

module.exports = { Queue, counter, main };
