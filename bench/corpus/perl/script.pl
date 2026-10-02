#!/usr/bin/env perl
use strict;
use warnings;

my %config = (
    host    => $ENV{APP_HOST} || 'localhost',
    ports   => [8080, 8081],
    nested  => { a => { b => { c => [1, [2, [3]]] } } },
);

my @sorted = sort { $a->{n} <=> $b->{n} } map { { n => $_ * 2 } } (3, 1, 2);

my $add = sub { my ($x, $y) = @_; return $x + $y };

for my $i (0 .. $#sorted) {
    next if $i == 1;
    printf "%d => %d\n", $i, $add->($sorted[$i]{n}, 1);
}

local $_ = 'one two three';
my @words = split /\s+/;
print scalar(@words), "\n" unless @words < 3;

open(my $fh, '<', '/dev/null') or die "cannot open: $!";
while (my $line = <$fh>) {
    chomp $line;
}
close $fh;
