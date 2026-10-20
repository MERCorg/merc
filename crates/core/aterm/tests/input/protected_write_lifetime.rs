use merc_aterm::ATerm;
use merc_aterm::ATermRef;
use merc_aterm::Protected;
use merc_aterm::Symbol;
use merc_aterm::Term;

// A term removed from the container is no longer protected, so it must not outlive the write
// access that removed it: after that a garbage collection may free it.
fn main() {
    let mut container: Protected<Vec<ATermRef<'static>>> = Protected::new(Vec::new());
    let a = ATerm::constant(&Symbol::new("a", 0));
    container.write(|write| {
        let t = unsafe { write.protect(&a) };
        write.push(t);
    });

    let t = container.write(|write| write.pop().unwrap());
    println!("Term: {:?}", t);

    // Copying an element out and then removing it must not work either.
    let t = container.write(|write| {
        let t = write[0].copy();
        write.clear();
        t
    });
    println!("Term: {:?}", t);
}
