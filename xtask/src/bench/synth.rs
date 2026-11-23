//! Deterministic synthetic plain-Java project generator (`plain-small`).

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

/// Summary of a generated `plain-small` corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthSummary {
    pub root: PathBuf,
    pub total_files: usize,
    pub total_lines: usize,
}

struct GeneratedFile {
    rel_path: String,
    contents: String,
}

/// Generates ~150 valid plain-Java files under `dest/src/` with zero build files.
///
/// Includes:
/// - `Chain0`..`Chain8` (8-level inheritance chain overriding `compute(int)`)
/// - `RootIface`, `LeftIface`, `RightIface`, `DiamondImpl` (diamond interface graph)
/// - `Tag`, `TagAlpha`, `TagBeta`, `TagGamma` (simple 3-impl interface)
/// - `Handler` + `Handler01`..`Handler50` (50 concrete implementations)
/// - `GenericBox<T>`, `User`, `Service`, `LargeFile` (~1,500+ LOC), `App`
/// - `Record01`..`Record77` (domain value classes)
pub fn generate_plain_small(dest: &Path) -> io::Result<SynthSummary> {
    let src_dir = dest.join("src");
    if src_dir.exists() {
        fs::remove_dir_all(&src_dir)?;
    }
    fs::create_dir_all(&src_dir)?;

    let mut files = Vec::with_capacity(150);
    push_chain_files(&mut files);
    push_diamond_files(&mut files);
    push_simple_files(&mut files);
    push_wide_files(&mut files);
    push_app_files(&mut files);
    push_domain_files(&mut files);

    let mut total_lines = 0usize;
    for file in &files {
        let full_path = dest.join(&file.rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full_path, &file.contents)?;
        total_lines += file.contents.lines().count();
    }

    Ok(SynthSummary {
        root: dest.to_path_buf(),
        total_files: files.len(),
        total_lines,
    })
}

fn push_chain_files(files: &mut Vec<GeneratedFile>) {
    files.push(GeneratedFile {
        rel_path: "src/bench/chain/Chain0.java".to_string(),
        contents: "\
package bench.chain;

public class Chain0 {
    protected int seed;

    public Chain0(int seed) {
        this.seed = seed;
    }

    public int compute(int x) {
        return seed + x;
    }

    public String label() {
        return \"Chain0\";
    }
}
"
        .to_string(),
    });

    for level in 1..=8 {
        let prev = level - 1;
        let contents = format!(
            "\
package bench.chain;

public class Chain{level} extends Chain{prev} {{
    public Chain{level}(int seed) {{
        super(seed);
    }}

    @Override
    public int compute(int x) {{
        return super.compute(x) + {level};
    }}
}}
"
        );
        files.push(GeneratedFile {
            rel_path: format!("src/bench/chain/Chain{level}.java"),
            contents,
        });
    }
}

fn push_diamond_files(files: &mut Vec<GeneratedFile>) {
    files.push(GeneratedFile {
        rel_path: "src/bench/diamond/RootIface.java".to_string(),
        contents: "\
package bench.diamond;

public interface RootIface {
    int rootValue();
}
"
        .to_string(),
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/diamond/LeftIface.java".to_string(),
        contents: "\
package bench.diamond;

public interface LeftIface extends RootIface {
    int leftValue();
}
"
        .to_string(),
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/diamond/RightIface.java".to_string(),
        contents: "\
package bench.diamond;

public interface RightIface extends RootIface {
    int rightValue();
}
"
        .to_string(),
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/diamond/DiamondImpl.java".to_string(),
        contents: "\
package bench.diamond;

public class DiamondImpl implements LeftIface, RightIface {
    @Override
    public int rootValue() {
        return 1;
    }

    @Override
    public int leftValue() {
        return 2;
    }

    @Override
    public int rightValue() {
        return 3;
    }
}
"
        .to_string(),
    });
}

fn push_simple_files(files: &mut Vec<GeneratedFile>) {
    files.push(GeneratedFile {
        rel_path: "src/bench/simple/Tag.java".to_string(),
        contents: "\
package bench.simple;

public interface Tag {
    String tagName();
}
"
        .to_string(),
    });

    for name in ["Alpha", "Beta", "Gamma"] {
        let contents = format!(
            "\
package bench.simple;

public class Tag{name} implements Tag {{
    @Override
    public String tagName() {{
        return \"{name}\";
    }}
}}
"
        );
        files.push(GeneratedFile {
            rel_path: format!("src/bench/simple/Tag{name}.java"),
            contents,
        });
    }
}

fn push_wide_files(files: &mut Vec<GeneratedFile>) {
    files.push(GeneratedFile {
        rel_path: "src/bench/wide/Handler.java".to_string(),
        contents: "\
package bench.wide;

public interface Handler {
    String handle(String input);
    int priority();
}
"
        .to_string(),
    });

    for idx in 1..=50 {
        let contents = format!(
            "\
package bench.wide;

public class Handler{idx:02} implements Handler {{
    @Override
    public String handle(String input) {{
        return \"h{idx:02}:\" + input;
    }}

    @Override
    public int priority() {{
        return {idx};
    }}
}}
"
        );
        files.push(GeneratedFile {
            rel_path: format!("src/bench/wide/Handler{idx:02}.java"),
            contents,
        });
    }
}

fn push_app_files(files: &mut Vec<GeneratedFile>) {
    files.push(GeneratedFile {
        rel_path: "src/bench/app/GenericBox.java".to_string(),
        contents: "\
package bench.app;

import java.util.function.Function;

public class GenericBox<T> {
    private final T value;

    public GenericBox(T value) {
        this.value = value;
    }

    public T getValue() {
        return value;
    }

    public <U> GenericBox<U> map(Function<T, U> fn) {
        return new GenericBox<>(fn.apply(value));
    }
}
"
        .to_string(),
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/app/User.java".to_string(),
        contents: "\
package bench.app;

public class User {
    private final int id;
    private final String name;
    private final String email;

    public User(int id, String name, String email) {
        this.id = id;
        this.name = name;
        this.email = email;
    }

    public int getId() {
        return id;
    }

    public String getName() {
        return name;
    }

    public String getEmail() {
        return email;
    }

    public String displayName() {
        return name + \" <\" + email + \">\";
    }

    public String format() {
        return displayName();
    }

    public String format(String prefix) {
        return prefix + \": \" + displayName();
    }

    public String format(String prefix, boolean upper) {
        String base = format(prefix);
        return upper ? base.toUpperCase() : base;
    }
}
"
        .to_string(),
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/app/Service.java".to_string(),
        contents: "\
package bench.app;

import bench.chain.Chain0;
import bench.chain.Chain8;
import bench.diamond.DiamondImpl;
import bench.diamond.RootIface;
import bench.simple.Tag;
import bench.simple.TagAlpha;
import bench.wide.Handler;
import bench.wide.Handler01;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public class Service {
    private final List<User> users = new ArrayList<>();
    private final Map<String, User> byName = new HashMap<>();
    private int nextId = 1;

    public User register(String name, String email) {
        User user = new User(nextId++, name, email);
        users.add(user);
        byName.put(name, user);
        int score = computeScore(user);
        if (score < 0) {
            return null;
        }
        return user;
    }

    private int computeScore(User user) {
        String label = user.getName();
        int len = label.length();
        return len + user.getId();
    }

    public List<String> summarizeAll() {
        List<String> out = new ArrayList<>();
        for (User u : users) {
            String formatted = u.format(\"user\", true);
            out.add(formatted);
        }
        return out;
    }

    public String firstDisplayName() {
        if (users.isEmpty()) {
            return \"\";
        }
        var first = users.get(0);
        String label = first.displayName();
        return label;
    }

    public String inspectBox(GenericBox<User> box) {
        String direct = box.getValue().displayName();
        return direct;
    }

    public int evaluateHierarchy(Chain0 chain, RootIface root, Handler handler, Tag tag) {
        int base = chain.compute(10);
        int rootVal = root.rootValue();
        int prio = handler.priority();
        String t = tag.tagName();
        return base + rootVal + prio + t.length();
    }

    public int runSample() {
        Chain8 c8 = new Chain8(5);
        DiamondImpl diamond = new DiamondImpl();
        Handler h1 = new Handler01();
        Tag tag = new TagAlpha();
        int total = evaluateHierarchy(c8, diamond, h1, tag);
        var summary = summarizeAll();
        return total + summary.size();
    }

    public int bodyCheck(int delta) {
        int acc = 10;
        acc += delta;
        return acc;
    }

    public String signatureCheck(String input) {
        return input.trim();
    }

    public int scopeCheck(int argVal) {
        int localVal = argVal + 1;
        return localVal;
    }

    public int autoImportCheck() {
        int seedVal = 42;
        return seedVal;
    }

    public Handler makeAnonymousHandler() {
        return new Handler() {
            @Override
            public String handle(String input) {
                return input;
            }

            @Override
            public int priority() {
                return 0;
            }
        };
    }
}
"
        .to_string(),
    });

    let mut large = String::with_capacity(64 * 1024);
    large.push_str(
        "\
package bench.app;

public class LargeFile {
    private int accumulator;

    public LargeFile(int accumulator) {
        this.accumulator = accumulator;
    }

    public int getAccumulator() {
        return accumulator;
    }

",
    );
    for idx in 1..=250 {
        let _ = writeln!(
            large,
            "    public int computeBatch{idx:03}(int x, int y) {{\n        int left = x + {idx};\n        int right = y * 2;\n        accumulator += left - right;\n        return accumulator + left + right;\n    }}\n"
        );
    }
    large.push_str("}\n");

    files.push(GeneratedFile {
        rel_path: "src/bench/app/LargeFile.java".to_string(),
        contents: large,
    });

    files.push(GeneratedFile {
        rel_path: "src/bench/app/App.java".to_string(),
        contents: "\
package bench.app;

import bench.chain.Chain8;
import bench.diamond.DiamondImpl;
import bench.simple.TagAlpha;
import bench.wide.Handler01;

public class App {
    public static void main(String[] args) {
        Service service = new Service();
        User alice = service.register(\"Alice\", \"alice@example.com\");
        GenericBox<User> box = new GenericBox<>(alice);
        String display = service.inspectBox(box);
        Chain8 chain = new Chain8(3);
        DiamondImpl diamond = new DiamondImpl();
        Handler01 handler = new Handler01();
        TagAlpha tag = new TagAlpha();
        int score = service.evaluateHierarchy(chain, diamond, handler, tag);
        LargeFile large = new LargeFile(score);
        int batch = large.computeBatch001(display.length(), service.runSample());
        System.out.println(batch);
    }
}
"
        .to_string(),
    });
}

fn push_domain_files(files: &mut Vec<GeneratedFile>) {
    for idx in 1..=77 {
        let contents = format!(
            "\
package bench.domain;

public class Record{idx:02} {{
    private final int code;
    private final String title;

    public Record{idx:02}(int code, String title) {{
        this.code = code;
        this.title = title;
    }}

    public int getCode() {{
        return code;
    }}

    public String getTitle() {{
        return title;
    }}

    public String describe() {{
        return \"Record{idx:02}[\" + code + \":\" + title + \"]\";
    }}
}}
"
        );
        files.push(GeneratedFile {
            rel_path: format!("src/bench/domain/Record{idx:02}.java"),
            contents,
        });
    }
}
