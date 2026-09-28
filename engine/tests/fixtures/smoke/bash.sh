#!/usr/bin/env bash

greet() {
  echo "hi $1"
}

main() {
  greet "x"
}

main "$@"
