/* Older idioms: prototypes, immediately invoked functions, labels and getters. */
var Legacy = (function () {
  "use strict";

  function Shape(name) {
    if (!(this instanceof Shape)) {
      return new Shape(name);
    }
    this.name = name;
  }

  Shape.prototype.describe = function () {
    return "shape " + this.name;
  };

  function Square(side) {
    Shape.call(this, "square");
    this.side = side;
  }

  Square.prototype = Object.create(Shape.prototype);
  Square.prototype.constructor = Square;
  Object.defineProperty(Square.prototype, "area", {
    get: function () {
      return this.side * this.side;
    },
  });

  function scan(grid) {
    var found = null;
    rows: for (var i = 0; i < grid.length; i++) {
      for (var j = 0; j < grid[i].length; j++) {
        if (grid[i][j] === 0) {
          found = [i, j];
          break rows;
        }
      }
    }
    return found;
  }

  var table = { "key with spaces": 1, nested: { deeper: { deepest: [1, [2, [3, [4]]]] } } };

  return { Shape: Shape, Square: Square, scan: scan, table: table };
})();

if (typeof module !== "undefined") {
  module.exports = Legacy;
}
