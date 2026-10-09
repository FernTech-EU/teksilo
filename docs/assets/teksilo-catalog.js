// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// The API catalogs' function names in FernTech teal (teksilo-catalog.css):
// the name in each `pub fn` heading of a page's API reference, and each
// function its "Public functions" tables link to. The generated pages stay
// plain Markdown; only the rendered book is coloured.
(function () {
    "use strict";

    var FN = /^(pub(?:\([^)]*\))?\s+(?:(?:const|async|unsafe|extern(?:\s+"[^"]*")?)\s+)*fn\s+)([A-Za-z_][A-Za-z0-9_]*)/;

    function markHeadings(root) {
        root.querySelectorAll("h4 code").forEach(function (code) {
            if (code.querySelector(".tk-fn")) {
                return;
            }
            var text = code.textContent;
            var m = FN.exec(text);
            if (!m) {
                return;
            }
            var name = document.createElement("span");
            name.className = "tk-fn";
            name.textContent = m[2];
            code.textContent = m[1];
            code.appendChild(name);
            code.appendChild(document.createTextNode(text.slice(m[0].length)));
        });
    }

    // "name(params)" in a contents table: the name teal, the parameters in
    // the text colour, as one link.
    function markCall(code) {
        if (code.querySelector(".tk-fn")) {
            return;
        }
        var m = /^([A-Za-z_][A-Za-z0-9_]*)([\s\S]*)$/.exec(code.textContent);
        if (!m) {
            return;
        }
        var name = document.createElement("span");
        name.className = "tk-fn";
        name.textContent = m[1];
        var params = document.createElement("span");
        params.className = "tk-params";
        params.textContent = m[2];
        code.textContent = "";
        code.appendChild(name);
        code.appendChild(params);
    }

    // The tables under "Public types" and "Public functions": aligned left,
    // and the functions' names marked.
    function markContents() {
        ["public-types", "public-functions"].forEach(function (id) {
            var heading = document.getElementById(id);
            if (!heading) {
                return;
            }
            for (var el = heading.nextElementSibling; el && el.tagName !== "H2"; el = el.nextElementSibling) {
                var tables = el.matches("table") ? [el] : Array.prototype.slice.call(el.querySelectorAll("table"));
                tables.forEach(function (table) {
                    table.classList.add("tk-contents");
                    if (id === "public-functions") {
                        table.querySelectorAll('a[href^="#"] > code').forEach(markCall);
                        // A row naming a group (Constructors, Methods...): no
                        // return type, its name in bold.
                        table.querySelectorAll("tbody tr").forEach(function (row) {
                            var cells = row.children;
                            if (cells.length === 2 && !cells[0].textContent.trim()
                                && cells[1].children.length === 1 && cells[1].children[0].tagName === "STRONG") {
                                row.classList.add("tk-group");
                            }
                        });
                    }
                });
            }
        });
    }

    function run() {
        var root = document.querySelector("main");
        if (!root) {
            return;
        }
        markHeadings(root);
        markContents();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", run);
    } else {
        run();
    }
})();
