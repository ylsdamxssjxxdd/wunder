pub const USER_PRIVATE_CONTAINER_ID: i32 = 0;
pub const DEFAULT_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MIN_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MAX_SANDBOX_CONTAINER_ID: i32 = 10;

pub fn normalize_sandbox_container_id(value: i32) -> i32 {
    value.clamp(MIN_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

pub fn normalize_workspace_container_id(value: i32) -> i32 {
    value.clamp(USER_PRIVATE_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_sandbox_container_id, normalize_workspace_container_id,
        DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID, MIN_SANDBOX_CONTAINER_ID,
        USER_PRIVATE_CONTAINER_ID,
    };

    #[test]
    fn normalize_sandbox_container_id_clamps_to_range() {
        assert_eq!(
            normalize_sandbox_container_id(MIN_SANDBOX_CONTAINER_ID - 1),
            MIN_SANDBOX_CONTAINER_ID
        );
        assert_eq!(
            normalize_sandbox_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_sandbox_container_id_keeps_default_in_range() {
        assert_eq!(
            normalize_sandbox_container_id(DEFAULT_SANDBOX_CONTAINER_ID),
            DEFAULT_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_workspace_container_id_allows_user_private_container() {
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID - 1),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }
}
