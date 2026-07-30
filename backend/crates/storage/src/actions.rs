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
    parse_reported_action_state(state).unwrap_or(ActionState::Running)
}

pub fn parse_reported_action_state(state: &str) -> Option<ActionState> {
    match state {
        "Running" | "running" => Some(ActionState::Running),
        "Completed" | "completed" => Some(ActionState::Completed),
        "Failed" | "failed" => Some(ActionState::Failed),
        "Cancelled" | "cancelled" => Some(ActionState::Cancelled),
        "TimedOut" | "timed_out" | "timedOut" => Some(ActionState::TimedOut),
        _ => None,
    }
}

pub(crate) fn action_status_allowed_source_states(next_state: &ActionState) -> Vec<ActionState> {
    match next_state {
        ActionState::Running => vec![ActionState::Running],
        ActionState::Completed => vec![ActionState::Running, ActionState::Completed],
        ActionState::Failed => vec![ActionState::Running, ActionState::Failed],
        ActionState::Cancelled => vec![ActionState::Running, ActionState::Cancelled],
        ActionState::TimedOut => vec![ActionState::Running, ActionState::TimedOut],
        ActionState::Queued | ActionState::WaitingApproval => Vec::new(),
    }
}
