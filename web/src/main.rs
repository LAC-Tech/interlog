use axum::{routing::get, Router};

/*
    GET /log				List of names of all logs in this cluster
    GET /log/:name/stats
    GET /log/:name/acqs		Acquaintances (ie, log addresses)
    GET /log/:name/head/:n	First n events
    GET /log/:name/tail/:n	Last n events

     PUT /log/:name			Creates a log
    POST /log/:name			Adds events to the log

    Sync protocol (binary):

    GET /log/:name/lc		Logical clock
    POST /log/:name/since	Responds with events (logical clock in body)
    POST /log/:name/remote

*/

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new().route("/", get(|| async { "Hello, World!" }));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
