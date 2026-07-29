use excalibur_domain::ActionState;

pub(crate) fn aggregate_action_state(
    target_count: usize,
    completed_count: usize,
    failed_count: usize,
    timed_out_count: usize,
    cancelled_count: usize,
    running_count: usize,
    waiting_count: usize,
) -> ActionState {
    if target_count == 0 {
        ActionState::Queued
    } else if completed_count == target_count {
        ActionState::Completed
    } else if failed_count > 0 {
        ActionState::Failed
    } else if timed_out_count > 0 {
        ActionState::TimedOut
    } else if cancelled_count > 0 {
        ActionState::Cancelled
    } else if running_count > 0 || completed_count > 0 {
        ActionState::Running
    } else if waiting_count > 0 {
        ActionState::WaitingApproval
    } else {
        ActionState::Queued
    }
}

pub fn map_terminal_action_state(state: &str) -> ActionState {
    match state {
        "Completed" | "completed" => ActionState::Completed,
        "Failed" | "failed" => ActionState::Failed,
        "Cancelled" | "cancelled" => ActionState::Cancelled,
        "TimedOut" | "timed_out" => ActionState::TimedOut,
        _ => ActionState::Running,
    }
}
