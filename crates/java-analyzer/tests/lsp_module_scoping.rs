mod common;

use common::{TempWorkspace, shutdown, start_server_with_root};
use java_analyzer::{cli::InspectArgs, inspect::run_with_io};
use serde_json::{Value, json};

const PUBLISH: &str = "textDocument/publishDiagnostics";

fn lsp_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Value {
    json!({
        "start": { "line": start_line, "character": start_char },
        "end": { "line": end_line, "character": end_char }
    })
}

#[test]
fn lsp_multi_module_scoping_and_main_test_isolation() {
    let ws = TempWorkspace::new();
    ws.write(
        ".java-analyzer.toml",
        "[modules.\"module-a\".dependencies]\ninclude = [\"module-b\"]\n",
    );

    let dep_service_uri = ws.write_uri(
        "module-b/src/main/java/com/b/DepService.java",
        "\
package com.b;
public class DepService {
    public String ping() { return \"b\"; }
}
",
    );
    ws.write_uri(
        "module-b/src/main/java/com/shared/SharedConfig.java",
        "\
package com.shared;
public class SharedConfig {
    public int fromB() { return 2; }
}
",
    );
    ws.write_uri(
        "module-c/src/main/java/com/c/SiblingOnly.java",
        "\
package com.c;
public class SiblingOnly {
    public int secret() { return 3; }
}
",
    );
    let mod_a_config_uri = ws.write_uri(
        "module-a/src/main/java/com/shared/SharedConfig.java",
        "\
package com.shared;
public class SharedConfig {
    public int fromA() { return 1; }
}
",
    );
    let mod_a_main_text = "\
package com.a;
import com.b.DepService;
import com.c.SiblingOnly;
import com.shared.SharedConfig;
public class AppMain {
    public String run() {
        TestHelper th = new TestHelper();
        int t = th.onlyTest();
        DepService dep = new DepService();
        int sib = new SiblingOnly().secret();
        int own = new SharedConfig().fromA();
        int other = new SharedConfig().fromB();
        return dep.ping();
    }
}
";
    let mod_a_main_uri = ws.write_uri("module-a/src/main/java/com/a/AppMain.java", mod_a_main_text);

    let mod_a_test_helper_text = "\
package com.a;
public class TestHelper {
    public int onlyTest() { return 42; }
}
";
    let mod_a_test_helper_uri = ws.write_uri(
        "module-a/src/test/java/com/a/TestHelper.java",
        mod_a_test_helper_text,
    );

    let mod_a_test_text = "\
package com.a;
import com.b.DepService;
import com.c.SiblingOnly;
public class AppMainTest {
    void verify() {
        String mainRes = new AppMain().run();
        int testRes = new TestHelper().onlyTest();
        String depRes = new DepService().ping();
        int sib = new SiblingOnly().secret();
    }
}
";
    let mod_a_test_uri = ws.write_uri("module-a/src/test/java/com/a/AppMainTest.java", mod_a_test_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&mod_a_main_uri, mod_a_main_text);
    client.open_uri(&mod_a_test_uri, mod_a_test_text);

    // 1. Main cannot jump to TestHelper or SiblingOnly, nor resolve module-b's shadowed SharedConfig.
    assert_eq!(client.definition_uri(&mod_a_main_uri, 6, 8), Value::Null);
    assert_eq!(client.definition_uri(&mod_a_main_uri, 7, 19), Value::Null);
    assert_eq!(client.definition_uri(&mod_a_main_uri, 9, 22), Value::Null);
    assert_eq!(client.definition_uri(&mod_a_main_uri, 9, 36), Value::Null);
    assert_eq!(client.definition_uri(&mod_a_main_uri, 11, 39), Value::Null);

    // Main CAN jump to its own SharedConfig.fromA() and module-b's DepService.ping().
    assert_eq!(
        client.definition_uri(&mod_a_main_uri, 10, 37),
        json!({ "uri": mod_a_config_uri, "range": lsp_range(2, 15, 2, 20) })
    );
    assert_eq!(
        client.definition_uri(&mod_a_main_uri, 12, 19),
        json!({ "uri": dep_service_uri, "range": lsp_range(2, 18, 2, 22) })
    );
    assert_eq!(
        client.hover_uri(&mod_a_main_uri, 12, 19)["contents"]["value"],
        json!("```java\nString com.b.DepService.ping()\n```")
    );

    // 2. Test CAN jump to Main (`AppMain.run`), Test (`TestHelper.onlyTest`), and module-b (`DepService.ping`),
    //    but still cannot jump to unrelated sibling module-c (`SiblingOnly`).
    assert_eq!(
        client.definition_uri(&mod_a_test_uri, 5, 39),
        json!({ "uri": mod_a_main_uri, "range": lsp_range(5, 18, 5, 21) })
    );
    assert_eq!(
        client.definition_uri(&mod_a_test_uri, 6, 39),
        json!({ "uri": mod_a_test_helper_uri, "range": lsp_range(2, 15, 2, 23) })
    );
    assert_eq!(
        client.definition_uri(&mod_a_test_uri, 7, 41),
        json!({ "uri": dep_service_uri, "range": lsp_range(2, 18, 2, 22) })
    );
    assert_eq!(client.definition_uri(&mod_a_test_uri, 8, 36), Value::Null);

    // 3. Opening a brand new Test buffer that wasn't on disk during initial probe respects Test scope.
    let new_test_path = ws.path().join("module-a/src/test/java/com/a/FreshTest.java");
    let new_test_uri = java_analyzer::convert::path_to_uri(&new_test_path)
        .expect("valid uri")
        .to_string();
    let new_test_text = "\
package com.a;
public class FreshTest {
    void check() {
        int v = new TestHelper().onlyTest();
    }
}
";
    client.open_uri(&new_test_uri, new_test_text);
    assert_eq!(
        client.definition_uri(&new_test_uri, 3, 33),
        json!({ "uri": mod_a_test_helper_uri, "range": lsp_range(2, 15, 2, 23) })
    );

    shutdown(client, handle);
}

#[test]
fn lsp_mrjar_java17_overlay_diagnostics_navigation_and_inspect() {
    let ws = TempWorkspace::new();
    ws.write(
        "pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
            <modelVersion>4.0.0</modelVersion>
            <groupId>com.example</groupId>
            <artifactId>mrjar-demo</artifactId>
            <version>1.0.0</version>
            <properties>
                <maven.compiler.release>8</maven.compiler.release>
                <maven.compiler.testRelease>21</maven.compiler.testRelease>
            </properties>
        </project>"#,
    );

    let base_helper_uri = ws.write_uri(
        "src/main/java/com/example/PlatformHelper.java",
        "\
package com.example;
public class PlatformHelper {
    public String mode() { return \"base\"; }
}
",
    );
    let base_caller_text = "\
package com.example;
public class BaseCaller {
    void use() {
        String m = new PlatformHelper().mode();
        int n = new PlatformHelper().java17Only();
    }
}
";
    let base_caller_uri = ws.write_uri("src/main/java/com/example/BaseCaller.java", base_caller_text);

    let overlay_helper_text = "\
package com.example;
public class PlatformHelper {
    public record Info(String label) {}
    public String mode() { return \"java17\"; }
    public int java17Only() { return 17; }
}
";
    let overlay_helper_uri = ws.write_uri(
        "src/main/java17/com/example/PlatformHelper.java",
        overlay_helper_text,
    );

    let overlay_caller_text = "\
package com.example;
public class OverlayCaller {
    public record LocalRec(int value) {}
    void use() {
        String m = new PlatformHelper().mode();
        int n = new PlatformHelper().java17Only();
    }
}
";
    let overlay_caller_uri = ws.write_uri(
        "src/main/java17/com/example/OverlayCaller.java",
        overlay_caller_text,
    );

    let test_caller_text = "\
package com.example;
public class PlatformHelperTest {
    void test() {
        String m = new PlatformHelper().mode();
        int n = new PlatformHelper().java17Only();
    }
}
";
    let test_caller_uri = ws.write_uri(
        "src/test/java/com/example/PlatformHelperTest.java",
        test_caller_text,
    );

    // Verify `java-analyzer inspect --json` and human output report `src/main/java17` release_override.
    let mut json_out = Vec::new();
    run_with_io(
        InspectArgs {
            workspace_dir: Some(ws.path().to_path_buf()),
            json: true,
            show_errors: true,
        },
        &mut json_out,
    )
    .expect("inspect --json should succeed");
    let json_val: Value = serde_json::from_slice(&json_out).expect("valid inspect json");
    assert_eq!(json_val["health"]["files_with_parse_errors"], 0);
    assert_eq!(json_val["package_mismatches"], json!([]));
    let sources = json_val["modules"][0]["sources"]
        .as_array()
        .expect("sources array");
    let java17_root = sources
        .iter()
        .find(|s| s["rel_path"] == "src/main/java17")
        .expect("src/main/java17 root in inspect json");
    assert_eq!(java17_root["scope"], "main");
    assert_eq!(java17_root["release_override"], 17);
    let base_root = sources
        .iter()
        .find(|s| s["rel_path"] == "src/main/java")
        .expect("src/main/java root in inspect json");
    assert_eq!(base_root["scope"], "main");
    assert_eq!(base_root["release_override"], Value::Null);

    let mut human_out = Vec::new();
    run_with_io(
        InspectArgs {
            workspace_dir: Some(ws.path().to_path_buf()),
            json: false,
            show_errors: true,
        },
        &mut human_out,
    )
    .expect("inspect human should succeed");
    let human_text = String::from_utf8(human_out).expect("utf8 output");
    assert!(
        human_text.contains("src/main/java17 [main, Java 17 overlay] (2 .java files)"),
        "expected Java 17 overlay tag in inspect output:\n{human_text}"
    );

    // Verify LSP diagnostics, goto_definition, and hover.
    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&overlay_helper_uri, overlay_helper_text);
    let diag1 = client.next_notification(PUBLISH);
    assert_eq!(diag1["uri"], json!(overlay_helper_uri));
    assert_eq!(
        diag1["diagnostics"],
        json!([]),
        "expected zero diagnostics on java17 PlatformHelper"
    );

    client.open_uri(&overlay_caller_uri, overlay_caller_text);
    let diag2 = client.next_notification(PUBLISH);
    assert_eq!(diag2["uri"], json!(overlay_caller_uri));
    assert_eq!(
        diag2["diagnostics"],
        json!([]),
        "expected zero diagnostics on java17 OverlayCaller"
    );

    client.open_uri(&base_caller_uri, base_caller_text);
    let _ = client.next_notification(PUBLISH);
    client.open_uri(&test_caller_uri, test_caller_text);
    let _ = client.next_notification(PUBLISH);

    // Base Java 8 caller resolves `mode()` to Java 8 `src/main/java`, and cannot resolve `java17Only()`.
    assert_eq!(
        client.definition_uri(&base_caller_uri, 3, 40),
        json!({ "uri": base_helper_uri, "range": lsp_range(2, 18, 2, 22) })
    );
    assert_eq!(client.definition_uri(&base_caller_uri, 4, 37), Value::Null);

    // Java 17 overlay caller resolves `mode()` and `java17Only()` to `src/main/java17`.
    assert_eq!(
        client.definition_uri(&overlay_caller_uri, 4, 40),
        json!({ "uri": overlay_helper_uri, "range": lsp_range(3, 18, 3, 22) })
    );
    assert_eq!(
        client.definition_uri(&overlay_caller_uri, 5, 37),
        json!({ "uri": overlay_helper_uri, "range": lsp_range(4, 15, 4, 25) })
    );
    assert_eq!(
        client.hover_uri(&overlay_caller_uri, 5, 37)["contents"]["value"],
        json!("```java\nint com.example.PlatformHelper.java17Only()\n```")
    );

    // Java 21 Test caller resolves `mode()` and `java17Only()` to the `src/main/java17` overlay.
    assert_eq!(
        client.definition_uri(&test_caller_uri, 3, 40),
        json!({ "uri": overlay_helper_uri, "range": lsp_range(3, 18, 3, 22) })
    );
    assert_eq!(
        client.definition_uri(&test_caller_uri, 4, 37),
        json!({ "uri": overlay_helper_uri, "range": lsp_range(4, 15, 4, 25) })
    );

    shutdown(client, handle);
}
