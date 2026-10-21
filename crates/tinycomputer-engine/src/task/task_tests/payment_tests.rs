//! Host-authorized continuation of an otherwise final payment checkpoint.

use super::*;
use tinycomputer_bus::agent::SecretHandle;

struct FixedPlan(String, Arc<Mutex<Vec<String>>>);

impl crate::planner::LanguageModel for FixedPlan {
    fn complete(&self, turns: &[crate::planner::Turn]) -> crate::planner::Completion {
        let answer = self.0.clone();
        let prompt = turns
            .iter()
            .map(|turn| turn.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.1.lock().unwrap().push(prompt);
        Box::pin(async move { Ok(answer) })
    }
}

fn payment_handles() -> BTreeMap<String, SecretHandle> {
    BTreeMap::from([
        (
            "card_number".to_owned(),
            SecretHandle {
                handle: "host-card-number".to_owned(),
                mask: "•••• 1111".to_owned(),
                kind: "payment_card".to_owned(),
            },
        ),
        (
            "expiry".to_owned(),
            SecretHandle {
                handle: "host-card-expiry".to_owned(),
                mask: "••/••".to_owned(),
                kind: "payment_card".to_owned(),
            },
        ),
        (
            "cvc".to_owned(),
            SecretHandle {
                handle: "host-card-cvc".to_owned(),
                mask: "•••".to_owned(),
                kind: "payment_card".to_owned(),
            },
        ),
    ])
}

fn payment_checkpoint() -> DesktopResponse {
    finished_run(
        FlowStopReason::StoppedBeforeDestructive,
        vec![step(
            "1",
            "stop_before",
            "paying now",
            StepOutcome::Gated,
            "found the payment control",
        )],
        &[],
        Some("Pay $12.50"),
    )
}

fn payment_approval() -> DesktopResponse {
    finished_run(
        FlowStopReason::StoppedBeforeDestructive,
        vec![step(
            "4",
            "stop_before",
            "paying now",
            StepOutcome::Gated,
            "found the payment control",
        )],
        &[],
        Some("Pay $12.50"),
    )
}

fn payment_task(replies: Vec<DesktopResponse>, planner: bool) -> (Tasks, Arc<Script>, TaskView) {
    payment_task_with_facts(replies, planner, BTreeMap::new())
}

fn payment_task_with_facts(
    replies: Vec<DesktopResponse>,
    planner: bool,
    facts: BTreeMap<String, String>,
) -> (Tasks, Arc<Script>, TaskView) {
    let (tasks, script) = controller(replies);
    let tasks = if planner {
        tasks.with_planner(crate::planner::Planner::new(Arc::new(FixedPlan(
            r#"{"app":"browser","steps":[{"enter":{"card number":"${card_number}"}},{"enter":{"expiry":"${expiry}"}},{"enter":{"cvc":"${cvc}"}},{"stop_before":"Pay now"}]}"#.to_owned(),
            script.planner_text.clone(),
        ))))
    } else {
        tasks
    };
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("buy the item in the current cart".to_owned()),
            flow: Some(flow(
                serde_json::json!({"app":"browser","steps":["read the cart"]}),
            )),
            facts,
            constraints: TaskConstraints {
                payment: PaymentMode::StopAtPayment,
                surfaces: vec![tinycomputer_bus::agent::SurfaceKind::Browser],
                origins: vec!["https://.shop.example".to_owned()],
                ..TaskConstraints::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    (tasks, script, started)
}

fn payment_request(
    id: TaskId,
    fact_handles: BTreeMap<String, SecretHandle>,
) -> ContinuePaymentRequest {
    ContinuePaymentRequest {
        id,
        fact_handles,
        checkout_context: "Current page: Checkout. Order total: $12.50 USD.".to_owned(),
    }
}

#[tokio::test]
async fn generic_continue_and_missing_handles_cannot_authorize_a_payment_checkpoint() {
    let (tasks, script, started) = payment_task(vec![payment_checkpoint()], false);
    let stopped = settle(&tasks, &started.id).await;
    assert!(matches!(
        stopped.status,
        TaskStatus::Checkpoint {
            continuable: false,
            ..
        }
    ));

    let ordinary = tasks.continue_task(ContinueTaskRequest {
        id: started.id.clone(),
        approve: Some(true),
        ..ContinueTaskRequest::default()
    });
    assert_eq!(code(&ordinary), "NOT_WAITING");

    let missing = tasks.continue_payment(payment_request(started.id.clone(), BTreeMap::new()));
    assert_eq!(code(&missing), "PAYMENT_TRANSITION_REFUSED");
    assert_eq!(settle(&tasks, &started.id).await.status, stopped.status);
    assert_eq!(script.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_cancelled_payment_checkpoint_cannot_be_reauthorized() {
    let (tasks, script, started) = payment_task(vec![payment_checkpoint()], false);
    let stopped = settle(&tasks, &started.id).await;
    assert!(matches!(stopped.status, TaskStatus::Checkpoint { .. }));
    assert_eq!(
        tasks.cancel(&started.id).data.unwrap().status,
        stopped.status
    );

    let denied = tasks.continue_payment(payment_request(started.id.clone(), payment_handles()));
    assert_eq!(code(&denied), "PAYMENT_TRANSITION_REFUSED");
    assert_eq!(script.requests.lock().unwrap().len(), 1);
    assert_eq!(*script.released.lock().unwrap(), [started.id]);
}

#[tokio::test]
async fn payment_transition_rejects_non_card_and_colliding_handles_without_mutation() {
    let (tasks, _script, started) = payment_task(vec![payment_checkpoint()], false);
    let stopped = settle(&tasks, &started.id).await;
    let mut invalid = payment_handles();
    invalid.get_mut("card_number").unwrap().kind = "customer_input".to_owned();
    let invalid = tasks.continue_payment(payment_request(started.id.clone(), invalid));
    assert_eq!(code(&invalid), "PAYMENT_TRANSITION_REFUSED");
    assert_eq!(settle(&tasks, &started.id).await.status, stopped.status);

    let (tasks, script, started) = payment_task(vec![payment_checkpoint()], false);
    let stopped = settle(&tasks, &started.id).await;
    let mut leaked = payment_request(started.id.clone(), payment_handles());
    leaked.checkout_context = "Card 4111 1111 1111 1111; total $12.50".to_owned();
    let leaked = tasks.continue_payment(leaked);
    assert_eq!(code(&leaked), "PAYMENT_TRANSITION_REFUSED");
    assert_eq!(settle(&tasks, &started.id).await.status, stopped.status);
    assert_eq!(script.requests.lock().unwrap().len(), 1);

    let (tasks, _script, started) = payment_task_with_facts(
        vec![payment_checkpoint()],
        false,
        BTreeMap::from([("card_number".to_owned(), "already-present".to_owned())]),
    );
    let stopped = settle(&tasks, &started.id).await;
    let started_with_collision =
        tasks.continue_payment(payment_request(started.id.clone(), payment_handles()));
    assert_eq!(code(&started_with_collision), "PAYMENT_TRANSITION_REFUSED");
    assert_eq!(settle(&tasks, &started.id).await.status, stopped.status);
}

#[tokio::test]
async fn a_held_payment_replans_the_same_task_and_keeps_its_current_page() {
    let (tasks, script, started) = payment_task(
        vec![
            payment_checkpoint(),
            payment_approval(),
            finished_run(FlowStopReason::Completed, vec![], &[], None),
        ],
        true,
    );
    let stopped = settle(&tasks, &started.id).await;
    assert!(matches!(stopped.status, TaskStatus::Checkpoint { .. }));

    let resumed = tasks
        .continue_payment(payment_request(started.id.clone(), payment_handles()))
        .data
        .unwrap();
    assert_eq!(resumed.id, started.id);
    let paused = settle(&tasks, &started.id).await;
    assert!(matches!(paused.status, TaskStatus::NeedsApproval { .. }));
    assert_eq!(
        script.constraints.lock().unwrap().last().unwrap().payment,
        PaymentMode::FillThenApprove
    );
    assert!(
        script.opened.lock().unwrap().is_empty(),
        "the retained checkout page must not be navigated away from"
    );
    let planned = script.planner_text.lock().unwrap().join("\n");
    assert!(!planned.contains("host-card-number"));
    assert!(!planned.contains("4111111111111111"));

    let approved = tasks
        .continue_task(ContinueTaskRequest {
            id: started.id.clone(),
            approve: Some(true),
            ..ContinueTaskRequest::default()
        })
        .data
        .unwrap();
    assert_eq!(approved.id, started.id);
    let paid = settle(&tasks, &started.id).await;
    assert!(
        matches!(paid.status, TaskStatus::Done { .. }),
        "{paid:?}; run_requests={:?}; replies_left={}",
        script.requests.lock().unwrap().len(),
        script.replies.lock().unwrap().len()
    );
    let requests = script.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        3,
        "one explicit approval causes one pay-control run"
    );
    assert!(requests[2].allow_destructive);
    let repeated = tasks.continue_payment(payment_request(started.id.clone(), payment_handles()));
    assert_eq!(code(&repeated), "PAYMENT_TRANSITION_REFUSED");
    assert!(matches!(
        tasks.report(&started.id).data.unwrap().view.status,
        TaskStatus::Done { .. }
    ));
}
