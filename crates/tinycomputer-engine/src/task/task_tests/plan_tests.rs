//! Tests for planning a plain-language task before running it.

use super::*;

struct WarmUsageRunner(Arc<Script>);

impl FlowRunner for WarmUsageRunner {
    fn run(
        &self,
        task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
    ) -> FlowFuture {
        self.0.run(task, constraints, request)
    }

    fn run_with_usage(
        &self,
        task: &TaskId,
        constraints: &TaskConstraints,
        request: RunFlowRequest,
        usage: Arc<crate::UsageRecorder>,
    ) -> FlowFuture {
        usage.record_call(crate::ModelRole::Jev, Some(5), Some(1), Some(2), Some(3));
        self.0.run(task, constraints, request)
    }

    fn warm_with_usage(
        &self,
        task: &TaskId,
        votes: u32,
        usage: Arc<crate::UsageRecorder>,
    ) -> super::super::PrepareFuture {
        usage.record_call(crate::ModelRole::Jev, Some(40), Some(10), Some(2), Some(12));
        self.0.warm(task, votes)
    }
}

/// A model that always answers with the same text.
struct Fixed(Result<String, String>);

impl crate::planner::LanguageModel for Fixed {
    fn complete(&self, _turns: &[crate::planner::Turn]) -> crate::planner::Completion {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

fn planned(replies: Vec<DesktopResponse>, answer: Result<&str, &str>) -> (Tasks, Arc<Script>) {
    let (tasks, script) = controller(replies);
    let model = Arc::new(Fixed(answer.map(str::to_owned).map_err(str::to_owned)));
    (
        tasks.with_planner(crate::planner::Planner::new(model)),
        script,
    )
}

#[tokio::test]
async fn a_plain_language_task_is_planned_then_run() {
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(r#"{"app": "Mail", "steps": [{"enter": {"recipient": "${email}"}}]}"#),
    );
    assert!(tasks.planner_configured());
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("email Sam".to_owned()),
            facts: BTreeMap::from([("email".to_owned(), "sam@example.com".to_owned())]),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    assert_eq!(started.summary, "Planning the task.");
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    let requests = script.requests.lock().unwrap();
    assert_eq!(requests[0].flow.app, "Mail");
    assert_eq!(requests[0].vars["email"], "sam@example.com");
    assert_eq!(
        tasks
            .report(&started.id)
            .data
            .unwrap()
            .flow
            .unwrap()
            .steps
            .len(),
        1
    );
    let plans = journaled(&script, "plan");
    assert_eq!(plans.len(), 1);
    assert_eq!(
        plans[0].0.as_ref(),
        Some(&started.id),
        "into the task's journal"
    );
    assert_eq!(plans[0].1["ok"], true);
    assert_eq!(plans[0].1["calls"], 1);
    assert_eq!(plans[0].1["steps"], 1);
    assert!(plans[0].1["wall_ms"].is_u64());
}

#[tokio::test]
async fn host_recovers_a_failed_planner_without_replacing_the_task() {
    let (tasks, script) = planned(
        vec![finished_run(
            FlowStopReason::Completed,
            vec![step(
                "1",
                "do",
                "open the shop home page",
                StepOutcome::Done,
                "",
            )],
            &[],
            None,
        )],
        Err("the configured planner is unavailable"),
    );
    *script.plan_recovery.lock().unwrap() = Some(crate::task::PlanRecoveryResult::Plan(flow(
        json!({"app":"browser","steps":["open the shop home page"]}),
    )));
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("find a blue wool sweater".to_owned()),
            constraints: TaskConstraints {
                surfaces: vec![tinycomputer_bus::agent::SurfaceKind::Browser],
                ..TaskConstraints::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();

    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    assert_eq!(script.requests.lock().unwrap().len(), 1);
    assert_eq!(script.plan_requests.lock().unwrap()[0].task, started.id);
}

#[tokio::test]
async fn a_planned_task_reports_usage_from_warmup_and_run() {
    let script = Arc::new(Script {
        replies: VecDeque::from([finished_run(FlowStopReason::Completed, vec![], &[], None)])
            .into(),
        ..Script::default()
    });
    let model = Arc::new(Fixed(Ok(
        r#"{"app":"Mail","steps":[{"enter":{"recipient":"${email}"}}]}"#.to_owned(),
    )));
    let tasks = Tasks::new(Arc::new(WarmUsageRunner(Arc::clone(&script))))
        .with_planner(crate::Planner::new(model));
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("email Sam".to_owned()),
            facts: BTreeMap::from([("email".to_owned(), "sam@example.com".to_owned())]),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();

    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    let report = tasks.report(&started.id).data.unwrap();
    assert_eq!(report.usage.jev.calls, 2);
    assert_eq!(report.usage.jev.input_tokens, Some(45));
    assert_eq!(report.usage.jev.cached_input_tokens, Some(11));
    assert_eq!(report.usage.jev.output_tokens, Some(4));
    assert_eq!(report.usage.jev.cost_usd_nanos, Some(15));
}

#[tokio::test]
async fn a_plan_that_needs_values_asks_and_a_failed_plan_says_so() {
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(r#"{"app": "browser", "steps": [{"enter": {"phone": "${phone}"}}]}"#),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("fill my phone".to_owned()),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    let waiting = settle(&tasks, &started.id).await;
    assert!(
        matches!(waiting.status, TaskStatus::NeedsInput { ref fields } if fields[0].name == "phone")
    );
    assert_eq!(
        *script.released.lock().unwrap(),
        std::slice::from_ref(&started.id),
        "a task waiting on a person holds no browser"
    );
    // An answer that still leaves a value missing keeps the task waiting:
    // no wait is over yet.
    assert!(
        tasks
            .continue_task(ContinueTaskRequest {
                id: started.id.clone(),
                ..ContinueTaskRequest::default()
            })
            .ok
    );
    assert_eq!(journaled(&script, "resume").len(), 0, "no resume yet");
    assert!(
        tasks
            .continue_task(ContinueTaskRequest {
                id: started.id.clone(),
                inputs: BTreeMap::from([("phone".to_owned(), "+91 98765 43210".to_owned())]),
                ..ContinueTaskRequest::default()
            })
            .ok
    );
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    assert_eq!(
        script.requests.lock().unwrap()[0].vars["phone"],
        "+91 98765 43210"
    );
    let resumes = journaled(&script, "resume");
    assert_eq!(
        resumes.len(),
        1,
        "the answer journals how long the task waited"
    );
    assert_eq!(resumes[0].1["state"], "needs_input");
    assert!(resumes[0].1["waited_ms"].is_u64());

    let (tasks, _) = planned(Vec::new(), Err("the model is down"));
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("anything".to_owned()),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Failed { ref reason, recoverable: true, .. } if reason == "the model is down"
    ));
}

#[tokio::test]
async fn plan_task_drafts_without_acting() {
    use tinycomputer_bus::agent::PlanTaskRequest;

    let request = PlanTaskRequest {
        task: "email Sam".to_owned(),
        ..PlanTaskRequest::default()
    };
    let (tasks, _) = controller(Vec::new());
    assert_eq!(code(&tasks.plan(&request).await), "PLANNER_NOT_CONFIGURED");
    let (tasks, script) = planned(
        Vec::new(),
        Ok(r#"{"app": "Mail", "steps": ["start a new email message"]}"#),
    );
    assert_eq!(tasks.plan(&request).await.data.unwrap().flow.app, "Mail");
    assert!(
        script.requests.lock().unwrap().is_empty(),
        "planning never runs anything"
    );
    let plans = journaled(&script, "plan");
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].0, None, "no task yet: a run of its own");
    assert_eq!(plans[0].1["ok"], true);
    let (tasks, script) = planned(Vec::new(), Err("down"));
    assert_eq!(code(&tasks.plan(&request).await), "PLAN_FAILED");
    let plans = journaled(&script, "plan");
    assert_eq!(plans[0].1["ok"], false);
    assert_eq!(plans[0].1["error"], "down");
}

#[tokio::test]
async fn a_browser_only_task_gets_its_browser_ready_while_it_is_planned() {
    use tinycomputer_bus::agent::{SurfaceKind, TaskConstraints};

    let mail = r#"{"app": "browser", "steps": [{"browse": "https://mail.test"}]}"#;
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(mail),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("open my mail".to_owned()),
            constraints: TaskConstraints {
                surfaces: vec![SurfaceKind::Browser],
                ..TaskConstraints::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    assert_eq!(
        *script.prepared.lock().unwrap(),
        std::slice::from_ref(&started.id)
    );

    // A task that may also use the desktop could be planned for either, so
    // nothing is opened before the plan says which.
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(mail),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("open my mail".to_owned()),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    settle(&tasks, &started.id).await;
    assert!(script.prepared.lock().unwrap().is_empty());

    // A plan that fails lets go of what was made ready for it.
    let (tasks, script) = planned(Vec::new(), Err("the model is down"));
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("open my mail".to_owned()),
            constraints: TaskConstraints {
                surfaces: vec![SurfaceKind::Browser],
                ..TaskConstraints::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Failed { .. }
    ));
    assert_eq!(
        *script.prepared.lock().unwrap(),
        std::slice::from_ref(&started.id)
    );
    assert_eq!(
        *script.released.lock().unwrap(),
        std::slice::from_ref(&started.id)
    );
}

#[tokio::test]
async fn a_task_warms_jev_while_it_is_planned_and_never_waits_for_it() {
    // The script's warm-up never ends: the task finishes all the same.
    let flow = r#"{"app": "Mail", "steps": ["start a new email message"]}"#;
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(flow),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("start an email".to_owned()),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    assert!(matches!(
        settle(&tasks, &started.id).await.status,
        TaskStatus::Done { .. }
    ));
    assert_eq!(*script.warmed.lock().unwrap(), [(started.id.clone(), 7)]);

    // Asked as many ways as the task's budget says.
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(flow),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("start an email".to_owned()),
            budget: tinycomputer_bus::agent::TaskBudget {
                votes: Some(3),
                ..tinycomputer_bus::agent::TaskBudget::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    settle(&tasks, &started.id).await;
    assert_eq!(*script.warmed.lock().unwrap(), [(started.id.clone(), 3)]);

    // A budget capping its Jev calls is not spent on warming.
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(flow),
    );
    let started = tasks
        .start(&StartTaskRequest {
            task: Some("start an email".to_owned()),
            budget: tinycomputer_bus::agent::TaskBudget {
                max_model_calls: Some(40),
                ..tinycomputer_bus::agent::TaskBudget::default()
            },
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    settle(&tasks, &started.id).await;
    assert!(script.warmed.lock().unwrap().is_empty());

    // A flow handed over whole is not planned, and not warmed.
    let (tasks, script) = controller(vec![finished_run(
        FlowStopReason::Completed,
        vec![],
        &[],
        None,
    )]);
    let started = tasks
        .start(&StartTaskRequest {
            flow: Some(serde_json::from_str(flow).unwrap()),
            ..StartTaskRequest::default()
        })
        .data
        .unwrap();
    settle(&tasks, &started.id).await;
    assert!(script.warmed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_browser_only_task_loads_the_page_it_names_while_it_is_planned() {
    use tinycomputer_bus::agent::{SurfaceKind, TaskConstraints};

    let mail = r#"{"app": "browser", "steps": [{"browse": "https://mail.test"}]}"#;
    let start = |tasks: &Tasks, task: &str, surfaces: Vec<SurfaceKind>| {
        tasks
            .start(&StartTaskRequest {
                task: Some(task.to_owned()),
                constraints: TaskConstraints {
                    surfaces,
                    ..TaskConstraints::default()
                },
                ..StartTaskRequest::default()
            })
            .data
            .unwrap()
    };
    let (tasks, script) = planned(
        vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
        Ok(mail),
    );
    let started = start(
        &tasks,
        "Go to https://mail.test and open my inbox.",
        vec![SurfaceKind::Browser],
    );
    settle(&tasks, &started.id).await;
    assert_eq!(
        *script.opened.lock().unwrap(),
        [(started.id.clone(), "https://mail.test".to_owned())]
    );
    assert_eq!(
        *script.prepared.lock().unwrap(),
        std::slice::from_ref(&started.id)
    );
    assert_eq!(
        script
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(**event, "prepare" | "open_page"))
            .collect::<Vec<_>>(),
        [&"prepare", &"open_page"],
        "in the browser opened for it"
    );

    // Two pages leave no one to start on; a task that may use the desktop
    // gets no browser early, nor a page.
    for (task, surfaces) in [
        (
            "Compare https://a.test with https://b.test",
            vec![SurfaceKind::Browser],
        ),
        ("Go to https://mail.test and open my inbox.", Vec::new()),
    ] {
        let (tasks, script) = planned(
            vec![finished_run(FlowStopReason::Completed, vec![], &[], None)],
            Ok(mail),
        );
        let started = start(&tasks, task, surfaces);
        settle(&tasks, &started.id).await;
        assert_eq!(script.opened.lock().unwrap().len(), 0, "{task}");
    }
}
