import json

from shop.shapes import Alpha, Widget, convert
from shop.util import compute_total
from shop.util import compute_total as total_alias


def local_helper():
    return 1


def factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)


def run_cases(thing):
    local_helper()  # A0: plain_local -> shop.main.local_helper
    compute_total(3)  # A0: imported -> shop.util.compute_total
    widget = Widget()  # A0: constructor -> shop.shapes.Widget
    widget.render()  # A0: method -> shop.shapes.Widget.render
    Widget.build_default()  # A0: static -> shop.shapes.Widget.build_default
    Alpha().draw()  # A0: same_short_name -> shop.shapes.Alpha.draw
    convert(1)  # A0: overload -> ambiguous
    json.dumps({})  # A0: external -> external
    factorial(4)  # A0: recursive -> shop.main.factorial
    total_alias(5)  # A0: alias -> shop.util.compute_total
    thing.render()  # A0: unresolved_receiver -> none
