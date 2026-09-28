export class Greeter {
  greet(name: string): string {
    return "hi " + name;
  }
}
export function make(): Greeter {
  return new Greeter();
}
