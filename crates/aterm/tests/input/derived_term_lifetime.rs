use merc_aterm::ATermInt;
use merc_aterm::ATermIntRef;
use merc_aterm::Protected;

// A reference copied from a reference must not outlive the owned term.
fn copy_of_ref_outlives_owner() {
    let r = {
        let owner = ATermInt::new(1);
        let r = owner.copy();
        r.copy()
    };
    println!("Term: {:?}", r);
}

// Copying through a borrow must not extend the lifetime of the referenced term.
fn copy_extends_lifetime<'a>(r: &ATermIntRef<'a>) -> ATermIntRef<'static> {
    r.copy()
}

// A copy of an element must not outlive the container that protects it.
fn copy_outlives_container() {
    let mut container: Protected<Vec<ATermIntRef<'static>>> = Protected::new(Vec::new());
    container.write(|write| {
        let t = unsafe { write.protect(&ATermInt::new(1)) };
        write.push(t.into());
    });

    let r: ATermIntRef<'static> = {
        let read = container.read();
        read[0].copy()
    };
    println!("Term: {:?}", r);
}

fn main() {
    copy_of_ref_outlives_owner();
    copy_outlives_container();
}
