"""A service over the models: cross-file imports, classes and exceptions."""

import logging
from contextlib import contextmanager

from models import Order, Status, fib, totals

log = logging.getLogger(__name__)


class OrderError(Exception):
    """Raised when an order cannot be processed."""


class Service:
    def __init__(self, orders=None):
        self.orders = list(orders or [])

    def complete(self, reference):
        for order in self.orders:
            if order.reference == reference:
                order.status = Status.DONE
                return order
        raise OrderError(f"no order {reference}")

    def summary(self):
        try:
            return totals(self.orders)
        except ZeroDivisionError as exc:
            log.warning("bad totals: %s", exc)
            raise OrderError("totals failed") from exc
        finally:
            log.debug("summary done")

    @contextmanager
    def transaction(self):
        snapshot = list(self.orders)
        try:
            yield self
        except OrderError:
            self.orders = snapshot
            raise

    def checksum(self):
        return fib(len(self.orders) % 20)


def build():
    service = Service([Order("A-1").add("sku-1", 2, price=3.5), Order("B-2")])
    with service.transaction() as s:
        s.complete("A-1")
    return service.summary(), service.checksum()
