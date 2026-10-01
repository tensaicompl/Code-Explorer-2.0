def parse(value):
    if not value:
        raise ValueError("empty")
    return int(value)
