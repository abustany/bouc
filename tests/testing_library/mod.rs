use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fantoccini::Client;
use fantoccini::actions::{InputSource, MouseActions, PointerAction};
use fantoccini::elements::{Element, ElementRef};
use serde_json::{Value, json};

pub const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

const TESTING_LIBRARY_DOM: &str = include_str!("testing-library-dom-10.4.1.min.js");
const WEB_ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";

pub struct User<'a> {
    client: &'a Client,
}

impl<'a> User<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self { client }
    }

    pub async fn click(&self, element: &Element) -> Result<()> {
        element.click().await.context("clicking element")
    }

    pub async fn hover(&self, element: &Element) -> Result<()> {
        self.client
            .perform_actions(MouseActions::new("mouse".to_owned()).then(
                PointerAction::MoveToElement {
                    element: element.clone(),
                    duration: None,
                    x: 0.0,
                    y: 0.0,
                },
            ))
            .await
            .context("hovering element")
    }

    pub async fn clear(&self, element: &Element) -> Result<()> {
        element.clear().await.context("clearing element")
    }

    pub async fn fill(&self, element: &Element, value: &str) -> Result<()> {
        self.clear(element).await?;
        element
            .send_keys(value)
            .await
            .context("typing into element")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NameMatch<'a> {
    Exact(&'a str),
    Contains(&'a str),
    AnyExact(&'a [&'a str]),
    AllContains(&'a [&'a str]),
}

impl<'a> NameMatch<'a> {
    fn values(self) -> Vec<&'a str> {
        match self {
            Self::Exact(value) | Self::Contains(value) => vec![value],
            Self::AnyExact(values) | Self::AllContains(values) => values.to_vec(),
        }
    }

    fn mode(self) -> &'static str {
        match self {
            Self::Exact(_) => "exact",
            Self::Contains(_) => "contains",
            Self::AnyExact(_) => "any-exact",
            Self::AllContains(_) => "all-contains",
        }
    }
}

const QUERY_ALL_BY_ROLE: &str = r#"
if (!globalThis.TestingLibraryDom) return null;
const root = arguments[0] ?? document.body;
const role = arguments[1];
const names = arguments[2];
const mode = arguments[3];
const normalize = value => value.trim().replace(/\s+/g, ' ').toLocaleLowerCase();
const expected = names.map(normalize);
const matchesName = name => {
    const actual = normalize(name);
    if (mode === 'any-exact') return expected.some(value => actual === value);
    if (mode === 'contains') return expected.some(value => actual.includes(value));
    return expected.every(value => actual.includes(value));
};
let options = {};
if (mode === 'exact') options = { name: names[0] };
if (mode === 'contains' || mode === 'any-exact' || mode === 'all-contains') {
    options = { name: matchesName };
}
return TestingLibraryDom.queryAllByRole(root, role, options);
"#;

#[derive(Clone, Copy)]
pub struct Queries<'a> {
    client: &'a Client,
    scope: Option<&'a Element>,
}

pub fn screen(client: &Client) -> Queries<'_> {
    Queries {
        client,
        scope: None,
    }
}

pub fn within<'a>(client: &'a Client, scope: &'a Element) -> Queries<'a> {
    Queries {
        client,
        scope: Some(scope),
    }
}

impl Queries<'_> {
    pub async fn find_by_role(self, role: &str, name: Option<NameMatch<'_>>) -> Result<Element> {
        find_by_role(self.client, self.scope, role, name).await
    }

    pub async fn query_by_role(
        self,
        role: &str,
        name: Option<NameMatch<'_>>,
    ) -> Result<Option<Element>> {
        query_by_role(self.client, self.scope, role, name).await
    }

    pub async fn query_all_by_role(
        self,
        role: &str,
        name: Option<NameMatch<'_>>,
    ) -> Result<Vec<Element>> {
        query_all_by_role(self.client, self.scope, role, name).await
    }

    pub async fn wait_for_role_count(
        self,
        role: &str,
        name: Option<NameMatch<'_>>,
        count: usize,
    ) -> Result<()> {
        wait_for_role_count(self.client, self.scope, role, name, count).await
    }
}

async fn query_all_by_role(
    client: &Client,
    scope: Option<&Element>,
    role: &str,
    name: Option<NameMatch<'_>>,
) -> Result<Vec<Element>> {
    let (names, mode) = name
        .map(|name| (name.values(), name.mode()))
        .unwrap_or_else(|| (Vec::new(), "none"));
    let args = vec![
        scope.map_or(Value::Null, |element| json!(element)),
        json!(role),
        json!(names),
        json!(mode),
    ];

    let mut result = eval(client, QUERY_ALL_BY_ROLE, args.clone()).await?;
    if result.is_null() {
        eval(client, TESTING_LIBRARY_DOM, vec![])
            .await
            .context("injecting Testing Library DOM")?;
        result = eval(client, QUERY_ALL_BY_ROLE, args).await?;
    }

    let values = result
        .as_array()
        .context("Testing Library role query did not return an array")?;
    values
        .iter()
        .map(|value| element_from_value(client, value))
        .collect()
}

fn element_from_value(client: &Client, value: &Value) -> Result<Element> {
    let id = value
        .get(WEB_ELEMENT_KEY)
        .and_then(Value::as_str)
        .context("Testing Library did not return a WebDriver element")?;
    Ok(Element::from_element_id(
        client.clone(),
        ElementRef::from(id.to_owned()),
    ))
}

async fn query_by_role(
    client: &Client,
    scope: Option<&Element>,
    role: &str,
    name: Option<NameMatch<'_>>,
) -> Result<Option<Element>> {
    let mut elements = query_all_by_role(client, scope, role, name).await?;
    if elements.len() > 1 {
        bail!(
            "found {} elements with role {role} and name {name:?}",
            elements.len()
        );
    }
    Ok(elements.pop())
}

async fn find_by_role(
    client: &Client,
    scope: Option<&Element>,
    role: &str,
    name: Option<NameMatch<'_>>,
) -> Result<Element> {
    let deadline = Instant::now() + WAIT_TIMEOUT;

    loop {
        if let Some(element) = query_by_role(client, scope, role, name).await? {
            return Ok(element);
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for role {role} and name {name:?}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_for_role_count(
    client: &Client,
    scope: Option<&Element>,
    role: &str,
    name: Option<NameMatch<'_>>,
    count: usize,
) -> Result<()> {
    let deadline = Instant::now() + WAIT_TIMEOUT;

    loop {
        let actual = query_all_by_role(client, scope, role, name).await?.len();
        if actual == count {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for {count} element(s) with role {role} and name {name:?}; found {actual}"
            );
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub async fn value(element: &Element) -> Result<String> {
    element
        .prop("value")
        .await
        .context("reading element value")?
        .context("element has no value")
}

pub async fn value_missing(element: &Element) -> Result<bool> {
    eval(
        &element.clone().client(),
        "return arguments[0].validity.valueMissing;",
        vec![json!(element)],
    )
    .await?
    .as_bool()
    .context("element validity is not a boolean")
}

pub async fn texts_in(client: &Client, scope: &Element, selector: &str) -> Result<Vec<String>> {
    let texts = eval(
        client,
        "return Array.from(arguments[0].querySelectorAll(arguments[1]), e => e.textContent);",
        vec![json!(scope), json!(selector)],
    )
    .await?;
    serde_json::from_value(texts).with_context(|| format!("reading the text of {selector}"))
}

pub async fn wait_for_animations(client: &Client, element: &Element) -> Result<()> {
    wait_for(
        client,
        "animations to settle",
        "return arguments[0].getAnimations({ subtree: true }).every(animation => \
             animation.playState === 'finished' || animation.playState === 'idle' \
         );",
        vec![json!(element)],
    )
    .await
}

pub async fn wait_for_attribute(
    client: &Client,
    element: &Element,
    attribute: &str,
    value: Option<&str>,
) -> Result<()> {
    wait_for(
        client,
        &format!("element to have {attribute}={value:?}"),
        "return arguments[0].getAttribute(arguments[1]) === arguments[2];",
        vec![json!(element), json!(attribute), json!(value)],
    )
    .await
}

pub async fn wait_for_count(client: &Client, selector: &str, count: usize) -> Result<()> {
    wait_for(
        client,
        &format!("{count} element(s) matching {selector}"),
        "return document.querySelectorAll(arguments[0]).length === arguments[1];",
        vec![json!(selector), json!(count)],
    )
    .await
}

pub async fn wait_for_count_in(
    client: &Client,
    scope: &Element,
    selector: &str,
    count: usize,
) -> Result<()> {
    wait_for(
        client,
        &format!("{count} displayed element(s) matching {selector}"),
        "return arguments[0].querySelectorAll(arguments[1]).length === arguments[2];",
        vec![json!(scope), json!(selector), json!(count)],
    )
    .await
}

pub async fn wait_for_text(client: &Client, selector: &str, text: &str) -> Result<()> {
    wait_for(
        client,
        &format!("{selector} to contain {text}"),
        "const e = document.querySelector(arguments[0]); \
         return !!e && e.textContent.includes(arguments[1]);",
        vec![json!(selector), json!(text)],
    )
    .await
}

async fn wait_for(
    client: &Client,
    description: &str,
    script: &str,
    args: Vec<Value>,
) -> Result<()> {
    let deadline = Instant::now() + WAIT_TIMEOUT;

    loop {
        if eval(client, script, args.clone()).await? == json!(true) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for {description}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub async fn eval(client: &Client, script: &str, args: Vec<Value>) -> Result<Value> {
    client
        .execute(script, args)
        .await
        .with_context(|| format!("evaluating {script}"))
}
