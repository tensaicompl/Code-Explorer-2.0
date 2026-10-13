import typing


class Widget:
    def render(self):
        return "widget"

    @staticmethod
    def build_default():
        return Widget()


class Alpha:
    def draw(self):
        return "alpha"


class Beta:
    def draw(self):
        return "beta"


@typing.overload
def convert(value: int) -> str: ...


@typing.overload
def convert(value: str) -> str: ...


def convert(value):
    return str(value)
