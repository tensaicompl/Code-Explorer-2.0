package Acme::Repo;

use strict;
use warnings;

use Exporter 'import';
our @EXPORT_OK = qw(make_repo);

sub new {
    my ($class, %args) = @_;
    my $self = { name => $args{name} };
    return bless $self, $class;
}

sub find {
    my ($self, $id) = @_;
    return $self->{name} . $id;
}

sub describe {
    my ($self) = @_;
    return $self->find(0);
}

sub make_repo {
    my ($name) = @_;
    return Acme::Repo->new(name => $name);
}

1;
