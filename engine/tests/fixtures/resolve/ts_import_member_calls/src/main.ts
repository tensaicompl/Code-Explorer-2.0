import { makeGreeter, Greeter } from './util';
export function main(): string {
  const g = makeGreeter();
  const h = new Greeter();
  return g.greet('a') + h.greet('b');
}
