pub mod shapes;
pub mod util;

use crate::shapes::{Alpha, Beta, Shape, Widget};
use crate::util::compute_total;
use crate::util::compute_total as total_alias;

macro_rules! twice {
    ($e:expr) => {
        $e + $e
    };
}

fn local_helper() -> u32 {
    1
}

fn factorial(n: u32) -> u32 {
    if n <= 1 {
        return 1;
    }
    n * factorial(n - 1)
}

pub fn run_cases() -> usize {
    local_helper(); // A0: plain_local -> src.lib.local_helper
    compute_total(3); // A0: imported -> src.util.compute_total
    let widget = Widget::new(); // A0: static -> src.shapes.Widget.new
    widget.render(); // A0: method -> src.shapes.Widget.render
    let alpha = Alpha;
    alpha.draw(); // A0: same_short_name -> src.shapes.Alpha.draw
    let beta = Beta;
    beta.area(); // A0: overload -> src.shapes.Beta.area
    let text = String::new(); // A0: external -> external
    factorial(4); // A0: recursive -> src.lib.factorial
    total_alias(5); // A0: alias -> src.util.compute_total
    let unknown = missing_function();
    unknown.render(); // A0: unresolved_receiver -> none
    assert_eq!(compute_total(1), 2); // A0: macro_token_tree -> src.util.compute_total
    let doubled = twice!(local_helper()); // A0: macro_invocation -> macro
    let _ = (alpha.area(), doubled);
    text.len()
}
