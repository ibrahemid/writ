pub mod authorized_paths;

pub use authorized_paths::{
    canonicalize_for_authorization, canonicalize_root, paths_equal_for_authorization,
    resolve_for_containment, AuthorizedPaths,
};
