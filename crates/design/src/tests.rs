use super::*;

const PAGE: &str = r#"import { Button } from "@/components/ui/button";

export function TasksPage({ tasks }: { tasks: string[] }) {
  return (
    <main className="mx-auto max-w-xl p-8">
      <header className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Tasks</h1>
        <Button variant="outline" size="sm">
          Open
        </Button>
      </header>
      <p>{tasks.length} open</p>
      <ul>
        {tasks.map((t) => (
          <li key={t} className="py-2">{t}</li>
        ))}
      </ul>
      <footer className={cn("mt-4", "text-sm")}>오늘의 할 일 · <b>3</b></footer>
    </main>
  );
}
"#;

const P: &str = "src/pages/tasks.tsx";

fn at(line: u32, column: u32) -> Place {
    Place { line, column }
}

/// The place of the first `needle` in PAGE (1-based, UTF-16 column).
fn find(source: &str, needle: &str) -> Place {
    let offset = source.find(needle).expect("needle");
    place_at(source, u32::try_from(offset).unwrap())
}

#[test]
fn a_location_splits_into_path_and_place() {
    assert_eq!(
        Place::parse_location("src/a:b.tsx:7:9"),
        Some(("src/a:b.tsx", at(7, 9)))
    );
    assert_eq!(Place::parse_location("src/a.tsx:0:1"), None);
    assert_eq!(Place::parse_location("nope"), None);
}

#[test]
fn a_heading_offers_its_text_and_classes() {
    let h1 = inspect(P, PAGE, find(PAGE, "<h1")).unwrap();
    assert_eq!(h1.name, "h1");
    assert!(!h1.component);
    assert_eq!(h1.text, Value::Literal("Tasks".into()));
    assert_eq!(
        h1.class_name,
        Value::Literal("text-2xl font-semibold".into())
    );
    assert_eq!(
        h1.moves,
        Moves {
            up: false,
            down: true
        },
        "first in the header"
    );
}

#[test]
fn a_component_offers_its_props_and_multiline_text() {
    let button = inspect(P, PAGE, find(PAGE, "<Button")).unwrap();
    assert!(button.component);
    assert_eq!(button.text, Value::Literal("Open".into()));
    assert_eq!(button.props["variant"], Value::Literal("outline".into()));
    assert_eq!(button.class_name, Value::Absent);
    let changed = apply(
        P,
        PAGE,
        find(PAGE, "<Button"),
        &Edit::Text("진행 중".into()),
    )
    .unwrap();
    assert!(
        changed
            .source
            .contains("<Button variant=\"outline\" size=\"sm\">\n          진행 중\n")
    );
}

#[test]
fn computed_values_are_refused_with_a_reason() {
    let p = inspect(P, PAGE, find(PAGE, "<p>")).unwrap();
    assert_eq!(p.text, Value::Computed, "text built with an expression");
    let li = inspect(P, PAGE, find(PAGE, "<li")).unwrap();
    assert_eq!(li.text, Value::Computed);
    assert!(li.in_list);
    let footer = inspect(P, PAGE, find(PAGE, "<footer")).unwrap();
    assert_eq!(footer.class_name, Value::Computed);
    assert_eq!(footer.text, Value::Absent);
    let error = apply(
        P,
        PAGE,
        find(PAGE, "<footer"),
        &Edit::ClassName("mt-6".into()),
    );
    assert!(matches!(error, Err(DesignError::Refused(r)) if r.contains("computed")));
    let error = apply(P, PAGE, find(PAGE, "<li"), &Edit::Text("x".into()));
    assert!(matches!(error, Err(DesignError::Refused(r)) if r.contains("computed")));
}

#[test]
fn classes_change_in_place_or_are_added() {
    let changed = apply(
        P,
        PAGE,
        find(PAGE, "<main"),
        &Edit::ClassName("mx-auto max-w-xl  p-10".into()),
    )
    .unwrap();
    assert!(
        changed
            .source
            .contains(r#"<main className="mx-auto max-w-xl p-10">"#)
    );
    let changed = apply(
        P,
        PAGE,
        find(PAGE, "<Button"),
        &Edit::ClassName("px-6".into()),
    )
    .unwrap();
    assert!(
        changed
            .source
            .contains(r#"<Button className="px-6" variant="outline""#)
    );
    let removed = apply(P, PAGE, find(PAGE, "<h1"), &Edit::ClassName(String::new())).unwrap();
    assert!(removed.source.contains("<h1>Tasks</h1>"));
}

#[test]
fn props_are_set_and_removed() {
    let place = find(PAGE, "<Button");
    let changed = apply(
        P,
        PAGE,
        place,
        &Edit::Prop {
            name: "variant".into(),
            value: Some("ghost".into()),
        },
    )
    .unwrap();
    assert!(
        changed
            .source
            .contains(r#"<Button variant="ghost" size="sm">"#)
    );
    let removed = apply(
        P,
        PAGE,
        place,
        &Edit::Prop {
            name: "size".into(),
            value: None,
        },
    )
    .unwrap();
    assert!(removed.source.contains(r#"<Button variant="outline">"#));
    assert!(
        apply(
            P,
            PAGE,
            place,
            &Edit::Prop {
                name: "onClick".into(),
                value: Some("x\" onMouseOver=\"y".into()),
            }
        )
        .unwrap()
        .source
        .contains(r#"onClick={"x\" onMouseOver=\"y"}"#)
    );
}

#[test]
fn text_with_markup_characters_stays_text() {
    let changed = apply(P, PAGE, find(PAGE, "<h1"), &Edit::Text("<b>{x}</b>".into())).unwrap();
    assert!(
        changed
            .source
            .contains(r#"<h1 className="text-2xl font-semibold">{"<b>{x}</b>"}</h1>"#)
    );
    // And it can be changed again from there.
    let again = apply(
        P,
        &changed.source,
        find(&changed.source, "<h1"),
        &Edit::Text("Done".into()),
    )
    .unwrap();
    assert!(again.source.contains(">Done</h1>"));
}

#[test]
fn siblings_swap_and_the_element_is_found_at_its_new_place() {
    let down = apply(P, PAGE, find(PAGE, "<h1"), &Edit::Move(1)).unwrap();
    let header =
        &down.source[down.source.find("<header").unwrap()..down.source.find("</header>").unwrap()];
    assert!(header.find("<Button").unwrap() < header.find("<h1").unwrap());
    assert_eq!(down.place, find(&down.source, "<h1"));
    let back = apply(P, &down.source, down.place, &Edit::Move(-1)).unwrap();
    assert_eq!(back.source, PAGE);
    assert_eq!(back.place, find(PAGE, "<h1"));
    let error = apply(P, PAGE, find(PAGE, "<h1"), &Edit::Move(-1));
    assert!(matches!(error, Err(DesignError::Refused(_))));
    // Children mixed with an expression keep their order.
    let li = inspect(P, PAGE, find(PAGE, "<li")).unwrap();
    assert_eq!(li.moves, Moves::default());
}

#[test]
fn columns_count_utf16_like_javascript() {
    let source = "const a = <p>오늘 😀</p>; const b = <i>x</i>;\n";
    let i = find(source, "<i>");
    // 10 + `<p>`3 + `오늘 😀`5 (😀 is two units) + `</p>; const b = `16 + 1
    assert_eq!(i, at(1, 35));
    assert_eq!(
        inspect("a.tsx", source, i).unwrap().text,
        Value::Literal("x".into())
    );
    assert_eq!(
        inspect("a.tsx", source, at(1, 34)),
        Err(DesignError::NotFound)
    );
}

#[test]
fn shadcn_variants_are_read_from_cva() {
    let button = r#"import { cva } from "class-variance-authority";
const buttonVariants = cva("inline-flex", {
  variants: {
    variant: { default: "bg-primary", outline: "border", ghost: "" },
    size: { default: "h-9", sm: "h-8", lg: "h-10", icon: "size-9" },
  },
  defaultVariants: { variant: "default", size: "default" },
});
export function Button() { return <button className={buttonVariants()} />; }
"#;
    let found = variants("button.tsx", button).unwrap();
    assert_eq!(found["variant"].options, ["default", "outline", "ghost"]);
    assert_eq!(found["size"].default.as_deref(), Some("default"));
    assert_eq!(
        imports(P, PAGE).unwrap()["Button"],
        "@/components/ui/button"
    );
}

#[test]
fn a_broken_file_is_a_syntax_error() {
    assert!(matches!(
        inspect(P, "const a = <p>;", at(1, 11)),
        Err(DesignError::Syntax(_))
    ));
}
