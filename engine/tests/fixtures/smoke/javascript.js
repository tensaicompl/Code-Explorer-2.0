class Greeter {
  greet(name) {
    return "hi " + name;
  }
}
function make() {
  return new Greeter();
}
module.exports = { make };
