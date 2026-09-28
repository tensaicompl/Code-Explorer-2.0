export class Counter {
  constructor() { this.n = 0; }
  inc() { this.n += 1; return this.n; }
}
export function makeCounter() { return new Counter(); }
