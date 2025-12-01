//! Tests for the Scheduler API
//!
//! These tests verify the scheduler functionality by starting the node
//! in a background task and interacting with the scheduler.

use constellation_node::{Data, Node, OverlapPolicy, Schedule, Scheduler, TaskStatus};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Helper to create a node and spawn its scheduler loop for testing.
/// Returns the scheduler and a counter for verification.
async fn create_test_scheduler() -> (Data<Scheduler>, Arc<AtomicU32>) {
    let counter = Arc::new(AtomicU32::new(0));

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(Arc::clone(&counter))
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background (this starts the scheduler loop)
    tokio::spawn(async move {
        let _ = node.start().await;
    });

    // Give scheduler loop time to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    (scheduler, counter)
}

#[tokio::test]
async fn test_schedule_after() {
    // Test that a one-shot task runs after the specified delay
    let (scheduler, counter) = create_test_scheduler().await;

    // Schedule task to run after 50ms
    let handle = scheduler
        .schedule(Schedule::after(Duration::from_millis(50)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Task shouldn't have run yet
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Wait for task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Task should have run exactly once
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Wait more - it shouldn't run again (one-shot)
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Check task info shows completed
    let info = scheduler.get(handle.id()).await;
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.run_count, 1);
    assert_eq!(info.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_schedule_interval() {
    // Test that an interval task runs multiple times
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule task to run every 30ms
    let handle = scheduler
        .schedule(Schedule::every(Duration::from_millis(30)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Wait for multiple runs
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should have run at least 3 times (0ms, 30ms, 60ms, 90ms)
    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 3, "Expected at least 3 runs, got {}", count);

    // Cancel to stop further runs
    let cancelled = handle.cancel().await;
    assert!(cancelled);
}

#[tokio::test]
async fn test_schedule_interval_initial_delay() {
    // Test that initial_delay is respected
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule with 100ms initial delay, then every 30ms
    let _handle = scheduler
        .schedule(
            Schedule::every_with_delay(Duration::from_millis(30), Duration::from_millis(100)),
            |ctx| async move {
                let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();

    // After 50ms, task shouldn't have run yet (initial delay is 100ms)
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // After another 100ms (150ms total), task should have run
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(counter.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn test_overlap_skip() {
    // Test that Skip policy prevents concurrent executions
    let concurrent = Arc::new(AtomicU32::new(0));
    let max_concurrent = Arc::new(AtomicU32::new(0));
    let concurrent_clone = Arc::clone(&concurrent);
    let max_concurrent_clone = Arc::clone(&max_concurrent);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(concurrent_clone)
        .data(max_concurrent_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule a slow task every 10ms with Skip policy (default)
    let _handle = scheduler
        .schedule(Schedule::every(Duration::from_millis(10)), |ctx| async move {
            let concurrent: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            let max_concurrent: Data<Arc<AtomicU32>> = ctx.extract().unwrap();

            let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(current, Ordering::SeqCst);

            // Simulate slow task
            tokio::time::sleep(Duration::from_millis(50)).await;

            concurrent.fetch_sub(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Wait for potential overlaps
    tokio::time::sleep(Duration::from_millis(200)).await;

    // With Skip policy, max concurrent should be 1
    assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_overlap_allow() {
    // Test that Allow policy permits concurrent executions
    let concurrent = Arc::new(AtomicU32::new(0));
    let max_concurrent = Arc::new(AtomicU32::new(0));
    let concurrent_clone = Arc::clone(&concurrent);
    let max_concurrent_clone = Arc::clone(&max_concurrent);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(concurrent_clone)
        .data(max_concurrent_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule a slow task every 10ms with Allow policy
    let _handle = scheduler
        .schedule_with_policy(
            Schedule::every(Duration::from_millis(10)),
            OverlapPolicy::Allow,
            |ctx| async move {
                let concurrent: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
                let max_concurrent: Data<Arc<AtomicU32>> = ctx.extract().unwrap();

                let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent.fetch_max(current, Ordering::SeqCst);

                // Simulate slow task
                tokio::time::sleep(Duration::from_millis(50)).await;

                concurrent.fetch_sub(1, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();

    // Wait for potential overlaps
    tokio::time::sleep(Duration::from_millis(100)).await;

    // With Allow policy, max concurrent should be > 1
    let max = max_concurrent.load(Ordering::SeqCst);
    assert!(max > 1, "Expected concurrent executions, got max {}", max);
}

#[tokio::test]
async fn test_task_list() {
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule multiple tasks
    let _h1 = scheduler
        .schedule_named("task1", Schedule::after(Duration::from_secs(60)), |_| async {})
        .await
        .unwrap();

    let _h2 = scheduler
        .schedule_named("task2", Schedule::every(Duration::from_secs(30)), |_| async {})
        .await
        .unwrap();

    let _h3 = scheduler
        .schedule(Schedule::after(Duration::from_secs(120)), |_| async {})
        .await
        .unwrap();

    // List all tasks
    // Note: 3 raft tasks (election_timeout, leader_heartbeat, apply_committed) are auto-scheduled
    let tasks = scheduler.list().await;
    assert_eq!(tasks.len(), 6); // 3 test tasks + 3 raft tasks

    // Check named tasks are present
    let names: Vec<_> = tasks.iter().filter_map(|t| t.name.as_ref()).collect();
    assert!(names.contains(&&"task1".to_string()));
    assert!(names.contains(&&"task2".to_string()));
}

#[tokio::test]
async fn test_task_cancel() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule interval task
    let handle = scheduler
        .schedule(Schedule::every(Duration::from_millis(20)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Let it run a bit
    tokio::time::sleep(Duration::from_millis(50)).await;
    let count_before = counter.load(Ordering::SeqCst);
    assert!(count_before >= 1);

    // Cancel the task
    let cancelled = handle.cancel().await;
    assert!(cancelled);

    // Wait and verify no more runs
    tokio::time::sleep(Duration::from_millis(100)).await;
    let count_after = counter.load(Ordering::SeqCst);

    // Should be the same or very close (maybe one more execution was in flight)
    assert!(
        count_after <= count_before + 1,
        "Task should have stopped after cancel"
    );

    // Check status is Cancelled
    let info = scheduler.get(handle.id()).await.unwrap();
    assert_eq!(info.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_task_kill() {
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule a task
    let handle = scheduler
        .schedule(Schedule::after(Duration::from_secs(60)), |_| async {})
        .await
        .unwrap();

    // Kill is sync - should return immediately
    handle.kill();

    // Give scheduler time to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Task should be cancelled
    let info = scheduler.get(handle.id()).await.unwrap();
    assert_eq!(info.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn test_task_schedules_task() {
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule a task that schedules another task
    let _handle = scheduler
        .schedule(Schedule::after(Duration::from_millis(10)), |ctx| async move {
            let scheduler = ctx.scheduler().expect("Scheduler should be extractable");

            // Schedule a child task
            scheduler
                .schedule(Schedule::after(Duration::from_millis(10)), |ctx| async move {
                    let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
                    counter.fetch_add(1, Ordering::SeqCst);
                })
                .await
                .ok();
        })
        .await
        .unwrap();

    // Wait for both tasks to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Child task should have run
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Should now have 5 tasks: 2 test tasks + 3 raft tasks (election_timeout, leader_heartbeat, apply_committed)
    let tasks = scheduler.list().await;
    assert_eq!(tasks.len(), 5);
}

#[tokio::test]
async fn test_extract_in_task() {
    // Test that TaskContext::extract() works like Node::extract()
    #[derive(Clone)]
    struct Config {
        value: String,
    }

    let config = Config {
        value: "test_value".to_string(),
    };

    let result = Arc::new(tokio::sync::Mutex::new(String::new()));
    let result_clone = Arc::clone(&result);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(config)
        .data(result_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    let _handle = scheduler
        .schedule(Schedule::after(Duration::from_millis(10)), |ctx| async move {
            let config: Data<Config> = ctx.extract().expect("Config should be extractable");
            let result: Data<Arc<tokio::sync::Mutex<String>>> = ctx.extract().unwrap();
            *result.lock().await = config.value.clone();
        })
        .await
        .unwrap();

    // Wait for task
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify extraction worked
    assert_eq!(*result.lock().await, "test_value");
}

#[tokio::test]
async fn test_cron_not_implemented() {
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Cron should return an error
    let result = scheduler
        .schedule(Schedule::Cron("0 0 * * *".to_string()), |_| async {})
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cron"));
}

#[tokio::test]
async fn test_random_interval() {
    // Test that RandomInterval runs multiple times with varying delays
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule task with random interval between 20-40ms
    let handle = scheduler
        .schedule(
            Schedule::random_interval(Duration::from_millis(20), Duration::from_millis(40)),
            |ctx| async move {
                let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();

    // Wait for multiple runs (with 20-40ms intervals, should get multiple runs in 200ms)
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should have run multiple times
    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 3, "Expected at least 3 runs, got {}", count);

    // Cancel to stop further runs
    let cancelled = handle.cancel().await;
    assert!(cancelled);
}

#[tokio::test]
async fn test_task_reset() {
    // Test that reset() restarts the timer
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule task to run after 100ms
    let handle = scheduler
        .schedule(Schedule::after(Duration::from_millis(100)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Wait 50ms, then reset
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task shouldn't have run yet");

    let reset = handle.reset().await;
    assert!(reset, "Reset should succeed");

    // Wait another 50ms - task still shouldn't have run (timer was reset)
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task shouldn't have run after reset");

    // Wait another 100ms - now it should have run
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1, "Task should have run after reset timeout");
}

#[tokio::test]
async fn test_random_interval_reset() {
    // Test that reset() on RandomInterval picks a new random delay
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule task with random interval between 100-150ms
    let handle = scheduler
        .schedule(
            Schedule::random_interval(Duration::from_millis(100), Duration::from_millis(150)),
            |ctx| async move {
                let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();

    // Reset multiple times before it can fire
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let reset = handle.reset().await;
        assert!(reset, "Reset should succeed");
    }

    // After 250ms of resets, task shouldn't have run
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task shouldn't have run due to resets");

    // Now wait for it to fire
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(counter.load(Ordering::SeqCst) >= 1, "Task should have run after stopping resets");

    handle.cancel().await;
}

#[tokio::test]
async fn test_reset_cancelled_task() {
    // Test that reset() on a cancelled task returns false
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule and then cancel
    let handle = scheduler
        .schedule(Schedule::after(Duration::from_secs(60)), |_| async {})
        .await
        .unwrap();

    let cancelled = handle.cancel().await;
    assert!(cancelled);

    // Reset on cancelled task should fail
    let reset = handle.reset().await;
    assert!(!reset, "Reset should fail on cancelled task");
}

#[tokio::test]
async fn test_handle_by_name() {
    // Test getting a handle by name and using it to reset
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .build()
        .unwrap();

    let scheduler: Data<Scheduler> = node.extract().unwrap();

    // Spawn node in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Schedule a named task
    let _handle = scheduler
        .schedule_named("my_task", Schedule::after(Duration::from_millis(100)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // Get handle by name
    let handle = scheduler.handle_by_name("my_task").await;
    assert!(handle.is_some(), "Should find task by name");

    // Non-existent name returns None
    let missing = scheduler.handle_by_name("nonexistent").await;
    assert!(missing.is_none(), "Should not find nonexistent task");

    // Use the handle to reset the timer
    let handle = handle.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task shouldn't have run yet");

    handle.reset_now();

    // After reset, wait 50ms more - still shouldn't have run (timer was reset)
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task shouldn't have run after reset");

    // Wait for it to actually fire
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1, "Task should have run");
}

#[tokio::test]
async fn test_builder_schedule() {
    // Test that tasks scheduled via builder work
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(counter_clone)
        .schedule(Schedule::after(Duration::from_millis(10)), |ctx| async move {
            let counter: Data<Arc<AtomicU32>> = ctx.extract().unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .unwrap();

    // Spawn node in background (this registers builder tasks and runs them)
    tokio::spawn(async move {
        let _ = node.start().await;
    });

    // Wait for task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Builder-scheduled task should have run
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
