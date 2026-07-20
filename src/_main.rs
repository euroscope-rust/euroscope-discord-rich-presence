// #[deny(unsafe_code)]
fn main() {
    println!("Hello world");
    // unsafe {
    //     let a: *mut i32 = std::ptr::null_mut();
    //     dbg!(*a);
    // }
}

// fn main() {
//     let a = Some(1_usize);
//     dbg!(a);
//     let b = a.unwrap();
//     dbg!(b);
// }

// fn main() {
//     let a = None;
//     dbg!(a);
//     let b: usize = a.unwrap(); // catches fire!
//     dbg!(b);
// }

// #[deny(clippy::unwrap_used)]
// fn main() {
//     let a = None;
//     dbg!(a);
//     let b: usize = a.unwrap(); // doesn't compile
//     dbg!(b);
// }

// fn main() {
//     let a = None;
//     dbg!(a);
//     if let Some(b) = a {
//         let b: usize = b;
//         dbg!(b);
//     } else {
//         println!("nothing was there :/");
//     }
// }

// fn some_failing_method() {
//     let a: Option<usize> = None;
//     a.unwrap();
// }
// fn main() {
//     some_failing_method();
// }

// fn some_failing_method() {
//     let a: Option<usize> = None;
//     a.unwrap(); // catches fire!
// }
// fn main() {
//     if let Err(err) = std::panic::catch_unwind(|| {
//         some_failing_method();
//     }) {
//         println!("we encountered an error, disabling plugin");
//         dbg!(err);
//     }
//     println!("euroscope resumes normal execution");
// }
