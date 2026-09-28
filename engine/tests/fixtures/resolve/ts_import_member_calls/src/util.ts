export class Greeter {
  greet(name: string): string { return 'hi ' + name; }
}
export function makeGreeter(): Greeter { return new Greeter(); }
