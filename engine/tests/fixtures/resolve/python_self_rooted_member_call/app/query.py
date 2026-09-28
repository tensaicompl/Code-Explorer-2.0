class Query:
    def __init__(self, compiler, parts):
        self.compiler = compiler
        self.parts = parts

    def run_owned(self):
        return self.compiler.apply_converters()

    def run_passed_in(self, other):
        return other.apply_converters()

    def run_builtin_name(self):
        return self.parts.extend([1])
