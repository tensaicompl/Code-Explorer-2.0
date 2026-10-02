package Corpus::Inventory;

use strict;
use warnings;
use utf8;
use Carp qw(croak);
use List::Util qw(sum0 max);

our $VERSION = '0.01';

sub new {
    my ($class, %args) = @_;
    my $self = bless {
        items => {},
        limit => $args{limit} // 100,
        log   => [],
    }, $class;
    return $self;
}

sub add {
    my ($self, $sku, $qty) = @_;
    croak "sku required" unless defined $sku && length $sku;
    $qty ||= 1;
    $self->{items}{$sku} += $qty;
    push @{ $self->{log} }, sprintf("added %d of %s", $qty, $sku);
    return $self;
}

sub total {
    my $self = shift;
    return sum0 values %{ $self->{items} };
}

sub largest {
    my ($self) = @_;
    my %items = %{ $self->{items} };
    my $max = max(values %items) // 0;
    return grep { $items{$_} == $max } sort keys %items;
}

sub report {
    my $self = shift;
    my $count = scalar keys %{ $self->{items} };
    my $text = <<"END";
Inventory report: $count skus
Total units: @{[ $self->total ]}
END
    $text =~ s/^\s+//gm;
    return $text;
}

sub parse_line {
    my ($line) = @_;
    if ($line =~ m{^(?<sku>[A-Z]{2}-\d+)\s*:\s*(?<qty>\d+)$}x) {
        return ($+{sku}, $+{qty});
    }
    return;
}

1;
