pub fn hash_password() {
    (encode_password)();
    hasher.digest();
    PasswordHasher::new();
    fn nested() {
        encode_password();
    }
    || {
        encode_password();
    };
    encode!();
    Point(1, 2);
}

pub unsafe extern "C" fn encode_password() {}

macro_rules! encode {
    () => {};
}

impl PasswordHasher {
    fn digest() {}
    fn new() {}
}

trait Digest {
    fn digest(&self) { encode_password() }
}

impl Digest for PasswordHasher {
    fn digest() {}
}

struct Point(u8, u8);

fn via_let() {
    let f = encode_password;
    f();
    let g = || {};
    g();
}

fn via_iso() {
    let f = encode_password;
    fn inner(f: fn()) {
        f();
    }
    f();
}

type Hasher = PasswordHasher;
impl Hasher {
    fn digest() {}
}
