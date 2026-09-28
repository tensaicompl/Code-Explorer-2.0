import { makeCounter } from './lib/counter.js';
export function run() {
  const c = makeCounter();
  return c.inc();
}
