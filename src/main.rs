#![feature(is_some_with)]

#[macro_use]
extern crate rocket;

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{stdout, Write};
use std::path::Path;

use rocket::{form, State};
use rocket::form::{Context, Contextual, Error, Form, FromForm};
use rocket::fs::{FileServer, relative, TempFile};
use rocket::http::Status;
use rocket::serde::{Deserialize, Serialize};
use rocket::serde::json::{Json, Value};
use rocket::serde::json::serde_json::json;
use rocket_dyn_templates::{context, Template};

use gazetteer::tree::{HashMapSearchTree, ResultSelection, SearchTree};
use gazetteer::util::read_lines;

#[cfg(test)]
mod rocket_test;

#[derive(Debug, FromForm)]
struct Submit<'v> {
    text: &'v str,
    file: TempFile<'v>,
    max_len: usize,
    max_deletes: usize,
    result_selection: ResultSelection,
}


#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Request<'r> {
    text: Cow<'r, str>,
    max_len: Option<usize>,
    max_deletes: Option<usize>,
    result_selection: Option<Cow<'r, str>>,
}

fn file_or_text<'v>(text: &'v str, file: &TempFile) -> form::Result<'v, String> {
    if !(
        text.len() > 1 || file.content_type().is_some_and(|t| t.is_text())) {
        Err(Error::validation("You must either enter text or upload a file!"))?
    } else if !text.is_empty() {
        Ok(String::from(text))
    } else {
        Ok(read_lines(file.path().unwrap().to_str().unwrap()).join(""))
    }
}


#[get("/")]
fn index() -> Template {
    Template::render("index", &Context::default())
}

#[post("/", data = "<form>")]
fn submit<'r>(mut form: Form<Contextual<'r, Submit<'r>>>, tree: &State<HashMapSearchTree>) -> (Status, Template) {
    let template = match form.value {
        Some(ref submission) => {
            // println!("submission: {:#?}", submission);
            match file_or_text(submission.text, &submission.file) {
                Ok(text) => {
                    let results = tree.search(
                        &text,
                        Option::from(submission.max_len),
                        Option::from(submission.max_deletes),
                        Option::from(&submission.result_selection),
                    );
                    // for result in results.iter() {
                    //     println!("{:?} ({},{}) -> {:?}", result.0, result.2, result.2, result.1)
                    // }
                    Template::render("success", context! {
                        text: text,
                        results: results,
                    })
                }
                Err(errs) => {
                    for err in errs {
                        form.context.push_error(err.with_name("file"));
                    }
                    Template::render("index", &form.context)
                }
            }
        }
        None => Template::render("index", &form.context),
    };

    (form.context.status(), template)
}

#[post("/tag", format = "json", data = "<request>")]
async fn tag(
    request: Json<Request<'_>>,
    tree: &State<HashMapSearchTree>,
) -> Value {
    let result_selection = match &request.result_selection {
        Some(sel) => match sel.as_ref() {
            "All" => &ResultSelection::All,
            "Last" => &ResultSelection::Last,
            "Longest" => &ResultSelection::Longest,
            _ => {
                println!("Unknown result selection method '{}', defaulting to 'Longest'", sel);
                &ResultSelection::Longest
            }
        },
        None => &ResultSelection::Longest
    };
    let results = tree.search(
        &request.text.to_lowercase(),
        request.max_len.or_else(|| Some(5 as usize)),
        request.max_deletes.or_else(|| Some(0 as usize)),
        Option::from(result_selection),
    );
    json!({
        "status": "ok",
        "results": results
    })
}

#[catch(500)]
fn tag_error() -> Value {
    json!({
        "status": "error",
        "reason": "An error occurred during tree search."
    })
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Config {
    filter_path: Option<String>,
    generate_abbrv: Option<bool>,
    generate_ngrams: Option<bool>,
    max_deletes: Option<usize>,
    corpora: HashMap<String, Corpus>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Corpus {
    path: String,
    filter_path: Option<String>,
    generate_abbrv: Option<bool>,
    generate_ngrams: Option<bool>,
    max_deletes: Option<usize>,
}

#[launch]
fn rocket() -> _ {
    let config = read_lines("resources/config.toml").join("\n");
    let config: Config = toml::from_str(&config).unwrap();

    let mut tree = HashMapSearchTree::default();
    let lines = config.filter_path.map_or_else(|| Vec::new(), |p| read_lines(Path::new(&p)));
    let filter_list = if lines.len() == 0 { None } else { Option::from(&lines) };

    for corpus in config.corpora.values() {
        let path: &String = &corpus.path;
        let generate_abbrv = corpus.generate_abbrv.unwrap_or_else(|| config.generate_abbrv.unwrap_or_else(|| false));
        let generate_ngrams = corpus.generate_ngrams.unwrap_or_else(|| config.generate_ngrams.unwrap_or_else(|| false));
        let max_deletes = corpus.max_deletes.unwrap_or_else(|| config.max_deletes.unwrap_or_else(|| 0));
        if let Some(_filter_path) = &corpus.filter_path {
            let _lines = read_lines(Path::new(&_filter_path));
            let _filter_list = Option::from(_lines);
            tree.load(&path, generate_ngrams, generate_abbrv, max_deletes, filter_list);
        } else {
            tree.load(&path, generate_ngrams, generate_abbrv, max_deletes, filter_list);
        }
    }
    let tree = tree;

    stdout().flush().expect("");
    println!("Finished loading gazetteer.");

    rocket::build()
        .mount("/", routes![index, submit, tag])
        .register("/tag", catchers![tag_error])
        .attach(Template::fairing())
        .mount("/", FileServer::from(relative!("/static")))
        .manage(tree)
}

// fn main() {
//     println!("Hello World")
//     // let (tree, symspell) = util::load_symspell("resources/taxa/Lichen/".to_string(), "resources/de-100k.txt");
//     // let string = String::from("Lyronna dolichobellum abc abc").to_lowercase();
//     // println!("{:?}", tree.traverse(string.clone().split(' ').collect::<VecDeque<&str>>()));
//     // let results = symspell.lookup_compound(string.as_str(), 2);
//     // if results.len() > 0 {
//     //     println!("{}", results[0].term);
//     //     println!("{:?}", tree.traverse(results[0].term.split(' ').collect::<VecDeque<&str>>()));
//     // }
// }