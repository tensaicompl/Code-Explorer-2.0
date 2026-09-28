package Demo::Greeter;
use strict;
use warnings;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub greet {
    my ($self, $name) = @_;
    return "hi $name";
}

1;
