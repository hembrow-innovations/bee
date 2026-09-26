mod hash;
use crate::hash;
pub use hash::hash_password;
use crate::hash::hash_password as hasher;

use crate::hash::hash_password;

fn via_use() {
    hash_password();
}

fn via_miss() {
    missing_imported();
}
