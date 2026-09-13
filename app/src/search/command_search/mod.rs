mod ai_queries;
mod env_var_collections;
mod history;
pub mod projects;
pub mod searcher;
pub mod settings;
pub mod view;
#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "Legacy cloud scaffolding remains compiled for tests but is not registered in Warp Local"
    )
)]
mod warp_ai;
mod workflows;
mod zero_state;
