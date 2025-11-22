use rs_coroutine_core::{CoroutineScope, Dispatcher, Dispatchers, CURRENT_SCOPE};
use rs_flow::{Flow, Suspending, suspend_block, flow_block};
use std::sync::Arc;

/// Example data structure for a JSON response
#[derive(Debug, Clone, serde::Deserialize)]
struct Post {
    #[serde(rename = "userId")]
    user_id: i32,
    id: i32,
    title: String,
    body: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct User {
    id: i32,
    name: String,
    email: String,
}

/// Fetch a single post from the API
async fn fetch_post(id: i32) -> Result<Post, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://jsonplaceholder.typicode.com/posts/{}", id);
    let response = reqwest::get(&url).await?;
    let post = response.json::<Post>().await?;
    println!("✓ Fetched post {}: {}", id, post.title);
    Ok(post)
}

/// Fetch a user from the API
async fn fetch_user(id: i32) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://jsonplaceholder.typicode.com/users/{}", id);
    let response = reqwest::get(&url).await?;
    let user = response.json::<User>().await?;
    println!("✓ Fetched user {}: {}", id, user.name);
    Ok(user)
}

/// Fetch multiple posts in parallel using async_task
async fn fetch_posts_parallel(
    scope: &CoroutineScope,
    post_ids: Vec<i32>,
) -> Vec<Post> {
    println!("\n📥 Fetching {} posts in parallel...", post_ids.len());

    // Start all fetch operations in parallel on the IO dispatcher
    let tasks: Vec<_> = post_ids
        .iter()
        .map(|&id| {
            scope.async_task(Dispatchers::io(), async move {
                fetch_post(id).await.ok()
            })
        })
        .collect();

    // Wait for all tasks to complete
    let mut results = Vec::new();
    for task in tasks {
        if let Some(post) = task.await_value().await {
            results.push(post);
        }
    }

    results
}

/// Create a flow that emits posts one by one
fn create_post_flow(post_ids: Vec<i32>) -> Flow<Post> {
    use futures::future::BoxFuture;
    let post_ids = Arc::new(post_ids);
    Flow::from_fn(move |mut collector| -> BoxFuture<'static, ()> {
        let post_ids = post_ids.clone();
        Box::pin(async move {
            for id in post_ids.iter() {
                match fetch_post(*id).await {
                    Ok(post) => collector.emit(post).await,
                    Err(e) => eprintln!("Error fetching post {}: {}", id, e),
                }
            }
        })
    })
}

/// Example 1: Simple web fetch using with_dispatcher
async fn example_simple_fetch(scope: &CoroutineScope) {
    println!("\n🌐 Example 1: Simple Web Fetch");
    println!("================================");

    // Fetch a post on the IO dispatcher
    let post = scope
        .with_dispatcher(Dispatchers::io(), async {
            fetch_post(1).await
        })
        .await;

    if let Ok(post) = post {
        println!("\n📄 Post Details:");
        println!("  Title: {}", post.title);
        println!("  Body: {}", post.body);
    }
}

/// Example 2: Parallel fetching with async_task
async fn example_parallel_fetch(scope: &CoroutineScope) {
    println!("\n🌐 Example 2: Parallel Fetch");
    println!("================================");

    let post_ids = vec![1, 2, 3, 4, 5];
    let posts = fetch_posts_parallel(scope, post_ids).await;

    println!("\n✅ Successfully fetched {} posts", posts.len());
    for post in posts.iter().take(3) {
        println!("  - {}", post.title);
    }
}

/// Example 3: Fetch user and their posts together
async fn example_combined_fetch(scope: &CoroutineScope) {
    println!("\n🌐 Example 3: Combined User + Posts Fetch");
    println!("==========================================");

    let user_id = 1;

    // Start both operations in parallel
    let user_task = scope.async_task(Dispatchers::io(), async move {
        fetch_user(user_id).await
    });

    let posts_task = scope.async_task(Dispatchers::io(), async move {
        let post_ids = vec![1, 2, 3];
        let mut posts = Vec::new();
        for id in post_ids {
            if let Ok(post) = fetch_post(id).await {
                if post.user_id == user_id {
                    posts.push(post);
                }
            }
        }
        posts
    });

    // Wait for both to complete
    let user = user_task.await_value().await;
    let posts = posts_task.await_value().await;

    if let Ok(user) = user {
        println!("\n👤 User: {} ({})", user.name, user.email);
        println!("📝 Their posts:");
        for post in posts {
            println!("  - {}", post.title);
        }
    }
}

/// Example 4: Using Flow to process fetched data
async fn example_flow_processing(scope: &CoroutineScope) {
    println!("\n🌐 Example 4: Flow Processing");
    println!("================================");

    let post_ids = vec![1, 2, 3, 4, 5];
    let flow = create_post_flow(post_ids);

    // Collect and process posts
    println!("\n📋 All fetched posts:");
    flow.collect(|post| async move {
        println!("  - [{}] {}", post.id, post.title);
    }).await;
}

/// Example 5: Using Suspending blocks
async fn example_suspending_blocks(scope: &CoroutineScope) {
    println!("\n🌐 Example 5: Suspending Blocks");
    println!("================================");

    // Create a suspending block that fetches a post
    let fetch_operation = suspend_block! {
        println!("🔄 Executing suspending block...");
        fetch_post(1).await.unwrap_or_else(|_| Post {
            user_id: 0,
            id: 0,
            title: "Error".to_string(),
            body: "Failed to fetch".to_string(),
        })
    };

    // Convert to a flow and collect
    let flow = fetch_operation.as_flow();

    flow.collect(|post| async move {
        println!("  Received: {}", post.title);
    }).await;

    println!("\n  Note: Suspending blocks are cold - they only execute when called!");
}

#[tokio::main]
async fn main() {
    println!("🚀 rs-coroutine Web Fetch Examples");
    println!("===================================\n");

    // Create the root coroutine scope with the default dispatcher
    let root_scope = Arc::new(CoroutineScope::new(Dispatchers::default()));

    // Run all examples within the coroutine scope context
    CURRENT_SCOPE
        .scope(root_scope.clone(), async move {
            let scope = &*root_scope;

            // Run examples
            example_simple_fetch(scope).await;
            example_parallel_fetch(scope).await;
            example_combined_fetch(scope).await;
            example_flow_processing(scope).await;
            example_suspending_blocks(scope).await;

            println!("\n✅ All examples completed!");
        })
        .await;
}
