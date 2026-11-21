mod common;

use common::{shutdown, start_server, start_server_with_root, TempWorkspace, URI};
use serde_json::{json, Value};

const PUBLISH: &str = "textDocument/publishDiagnostics";

fn lsp_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Value {
    json!({
        "start": { "line": start_line, "character": start_char },
        "end": { "line": end_line, "character": end_char }
    })
}

#[test]
fn goto_definition_within_single_file() {
    let (mut client, handle) = start_server();
    client.open(
        "\
class Calc {
    int base = 10;
    int helper(int v) { return v + 1; }
    int compute(int param) {
        int local = param + base;
        return helper(local);
    }
}
",
    );

    // `param` at line 4, col 20 -> declared at line 3, cols 20..25
    assert_eq!(
        client.definition_uri(URI, 4, 20),
        json!({ "uri": URI, "range": lsp_range(3, 20, 3, 25) })
    );

    // `base` at line 4, col 28 -> declared at line 1, cols 8..12
    assert_eq!(
        client.definition_uri(URI, 4, 28),
        json!({ "uri": URI, "range": lsp_range(1, 8, 1, 12) })
    );

    // `helper` at line 5, col 15 -> declared at line 2, cols 8..14
    assert_eq!(
        client.definition_uri(URI, 5, 15),
        json!({ "uri": URI, "range": lsp_range(2, 8, 2, 14) })
    );

    // `local` at line 5, col 22 -> declared at line 4, cols 12..17
    assert_eq!(
        client.definition_uri(URI, 5, 22),
        json!({ "uri": URI, "range": lsp_range(4, 12, 4, 17) })
    );

    shutdown(client, handle);
}

#[test]
fn goto_definition_across_files_in_workspace() {
    let ws = TempWorkspace::new();
    let helper_uri = ws.write_uri(
        "src/com/util/Helper.java",
        "\
package com.util;
public class Helper {
    public int value;
    public int work() { return value; }
}
",
    );
    let same_pkg_uri = ws.write_uri(
        "src/com/app/SamePkg.java",
        "\
package com.app;
public class SamePkg {
    public void ping() {}
}
",
    );
    let app_text = "\
package com.app;
import com.util.Helper;
public class App {
    void run(SamePkg peer) {
        Helper h = new Helper();
        int x = h.work();
        peer.ping();
    }
}
";
    let app_uri = ws.write_uri("src/com/app/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    // Only open App.java; Helper.java and SamePkg.java remain closed on disk.
    client.open_uri(&app_uri, app_text);

    // Import `Helper` at line 1, col 16 -> Helper.java line 1, cols 13..19
    assert_eq!(
        client.definition_uri(&app_uri, 1, 16),
        json!({ "uri": helper_uri, "range": lsp_range(1, 13, 1, 19) })
    );

    // Type `Helper` at line 4, col 8 -> Helper.java line 1, cols 13..19
    assert_eq!(
        client.definition_uri(&app_uri, 4, 8),
        json!({ "uri": helper_uri, "range": lsp_range(1, 13, 1, 19) })
    );

    // Method `h.work()` at line 5, col 18 -> Helper.java line 3, cols 15..19
    assert_eq!(
        client.definition_uri(&app_uri, 5, 18),
        json!({ "uri": helper_uri, "range": lsp_range(3, 15, 3, 19) })
    );

    // Hover on `h.work()` shows semantic signature from unopened Helper.java.
    let hover = client.hover_uri(&app_uri, 5, 18);
    assert_eq!(
        hover["contents"]["value"],
        json!("```java\nint com.util.Helper.work()\n```")
    );

    // Same-package type `SamePkg` at line 3, col 13 -> SamePkg.java line 1, cols 13..20
    assert_eq!(
        client.definition_uri(&app_uri, 3, 13),
        json!({ "uri": same_pkg_uri, "range": lsp_range(1, 13, 1, 20) })
    );

    // Same-package method `peer.ping()` at line 6, col 13 -> SamePkg.java line 2, cols 16..20
    assert_eq!(
        client.definition_uri(&app_uri, 6, 13),
        json!({ "uri": same_pkg_uri, "range": lsp_range(2, 16, 2, 20) })
    );

    shutdown(client, handle);
}

#[test]
fn goto_definition_reflects_unsaved_edits_and_reloads_disk_on_close() {
    let ws = TempWorkspace::new();
    let helper_uri = ws.write_uri(
        "src/com/util/Helper.java",
        "\
package com.util;
public class Helper {
    public int work() { return 1; }
}
",
    );
    let app_text = "\
package com.app;
import com.util.Helper;
public class App {
    void run() {
        Helper h = new Helper();
        int x = h.work();
    }
}
";
    let app_uri = ws.write_uri("src/com/app/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);
    let _app_diag = client.next_notification(PUBLISH);

    // Initially points to disk Helper.java line 2, cols 15..19.
    assert_eq!(
        client.definition_uri(&app_uri, 5, 18),
        json!({ "uri": helper_uri, "range": lsp_range(2, 15, 2, 19) })
    );

    // Open Helper.java with unsaved buffer shifting `work` to line 4, cols 16..20.
    client.open_uri(
        &helper_uri,
        "\
package com.util;
public class Helper {
    public int extraField;
    public long anotherField;
    public long work() { return 2L; }
}
",
    );
    let helper_open_diag = client.next_notification(PUBLISH);
    assert_eq!(helper_open_diag["uri"], json!(helper_uri));
    assert_eq!(helper_open_diag["diagnostics"], json!([]));

    assert_eq!(
        client.definition_uri(&app_uri, 5, 18),
        json!({ "uri": helper_uri, "range": lsp_range(4, 16, 4, 20) })
    );
    assert_eq!(
        client.hover_uri(&app_uri, 5, 18)["contents"]["value"],
        json!("```java\nlong com.util.Helper.work()\n```")
    );

    // Edit Helper.java in memory via didChange (with a trailing syntax error).
    client.change_uri(
        &helper_uri,
        2,
        "\
package com.util;
public class Helper {
    public String work() { return \"\"; }
    int broken
}
",
    );
    let helper_change_diag = client.next_notification(PUBLISH);
    assert_eq!(helper_change_diag["uri"], json!(helper_uri));
    assert_eq!(helper_change_diag["diagnostics"].as_array().unwrap().len(), 1);

    assert_eq!(
        client.definition_uri(&app_uri, 5, 18),
        json!({ "uri": helper_uri, "range": lsp_range(2, 18, 2, 22) })
    );
    assert_eq!(
        client.hover_uri(&app_uri, 5, 18)["contents"]["value"],
        json!("```java\nString com.util.Helper.work()\n```")
    );

    // Close Helper.java -> diagnostics clear and disk content is reloaded.
    client.close_uri(&helper_uri);
    let helper_close_diag = client.next_notification(PUBLISH);
    assert_eq!(helper_close_diag["uri"], json!(helper_uri));
    assert_eq!(helper_close_diag["diagnostics"], json!([]));

    assert_eq!(
        client.definition_uri(&app_uri, 5, 18),
        json!({ "uri": helper_uri, "range": lsp_range(2, 15, 2, 19) })
    );
    assert_eq!(
        client.hover_uri(&app_uri, 5, 18)["contents"]["value"],
        json!("```java\nint com.util.Helper.work()\n```")
    );

    shutdown(client, handle);
}

#[test]
fn goto_definition_on_whitespace_or_unknown_symbol_returns_null() {
    let (mut client, handle) = start_server();
    client.open("class Main { String s; }");

    // Whitespace
    assert_eq!(client.definition_uri(URI, 0, 5), Value::Null);
    // Keyword `class`
    assert_eq!(client.definition_uri(URI, 0, 1), Value::Null);
    // Unresolved external type `String`
    assert_eq!(client.definition_uri(URI, 0, 13), Value::Null);
    // Unknown file URI
    assert_eq!(
        client.definition_uri("file:///tmp/Missing.java", 0, 0),
        Value::Null
    );

    shutdown(client, handle);
}

#[test]
fn goto_definition_uses_utf16_columns_in_target_file() {
    let ws = TempWorkspace::new();
    let target_uri = ws.write_uri(
        "src/com/app/Target.java",
        "\
package com.app;
/* 😀 */ public class Target {}
",
    );
    let app_text = "\
package com.app;
public class App {
    /* 😀 */ Target field;
}
";
    let app_uri = ws.write_uri("src/com/app/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    // `Target` at line 2, UTF-16 col 13 (`    /* 😀 */ ` = 4 + 3 + 2 + 4 = 13)
    // -> Target.java line 1, UTF-16 cols 22..28 (`/* 😀 */ public class ` = 3 + 2 + 17 = 22).
    assert_eq!(
        client.definition_uri(&app_uri, 2, 13),
        json!({ "uri": target_uri, "range": lsp_range(1, 22, 1, 28) })
    );

    shutdown(client, handle);
}
