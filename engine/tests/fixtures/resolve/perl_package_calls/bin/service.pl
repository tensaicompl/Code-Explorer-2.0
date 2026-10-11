use strict;
use warnings;

use Acme::Repo qw(make_repo);

sub lookup {
    my ($id) = @_;
    my $repo = make_repo('users');
    return $repo->find($id);
}

sub direct {
    my $repo = Acme::Repo->new(name => 'direct');
    return $repo->find(1);
}

print lookup(3), "\n";
unknown_helper(2);
