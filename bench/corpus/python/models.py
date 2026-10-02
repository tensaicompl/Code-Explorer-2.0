"""Data models: dataclasses, typing, decorators, generators and pattern matching."""

from __future__ import annotations

import functools
import os
from dataclasses import dataclass, field
from enum import Enum
from typing import Generic, Iterator, TypeVar

T = TypeVar("T")


class Status(Enum):
    PENDING = "pending"
    DONE = "done"


@dataclass(frozen=True)
class Line:
    sku: str
    qty: int = 1
    price: float = 0.0

    @property
    def total(self) -> float:
        return self.qty * self.price


@dataclass
class Order:
    reference: str
    lines: list[Line] = field(default_factory=list)
    status: Status = Status.PENDING
    café: str = "noir"

    def add(self, sku: str, qty: int = 1, *, price: float = 0.0) -> "Order":
        if qty <= 0:
            raise ValueError(f"quantity must be positive, got {qty!r}")
        self.lines.append(Line(sku, qty, price))
        return self

    def __iter__(self) -> Iterator[Line]:
        yield from self.lines

    def describe(self) -> str:
        match self.status:
            case Status.PENDING if not self.lines:
                return "empty and pending"
            case Status.PENDING:
                return f"{len(self.lines)} lines pending"
            case Status.DONE:
                return "done"
        return "unknown"


class Box(Generic[T]):
    def __init__(self, value: T) -> None:
        self._value = value

    def map(self, fn):
        return Box(fn(self._value))

    @staticmethod
    def of(value: T) -> "Box[T]":
        return Box(value)

    @classmethod
    def empty(cls) -> "Box[None]":
        return cls(None)


def memoised(fn):
    cache: dict = {}

    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        key = (args, tuple(sorted(kwargs.items())))
        if key not in cache:
            cache[key] = fn(*args, **kwargs)
        return cache[key]

    return wrapper


@memoised
def fib(n: int) -> int:
    return n if n < 2 else fib(n - 1) + fib(n - 2)


def totals(orders: list[Order]) -> dict[str, float]:
    return {o.reference: sum(line.total for line in o) for o in orders if o.lines}


def config_home() -> str:
    return os.environ.get("HOME", "/tmp") + os.getenv("SUFFIX", "")


async def fetch_all(sources):
    results = []
    async for item in sources:
        if (n := len(item)) > 3:
            results.append((item, n))
    return results


NESTED = {"a": [1, {"b": (2, [3, {"c": {4, 5}}])}], "ñ": "\N{LATIN SMALL LETTER N WITH TILDE}"}
LAMBDAS = [lambda x, k=k: x * k for k in range(3)]
