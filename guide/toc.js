// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><a href="introduction.html">Introduction</a></li><li class="chapter-item expanded "><a href="fundamentals/index.html"><strong aria-hidden="true">1.</strong> Fundamentals</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="fundamentals/world.html"><strong aria-hidden="true">1.1.</strong> World</a></li><li class="chapter-item expanded "><a href="fundamentals/components.html"><strong aria-hidden="true">1.2.</strong> Components</a></li><li class="chapter-item expanded "><a href="fundamentals/query.html"><strong aria-hidden="true">1.3.</strong> Queries</a></li><li class="chapter-item expanded "><a href="fundamentals/systems.html"><strong aria-hidden="true">1.4.</strong> Systems</a></li><li class="chapter-item expanded "><a href="fundamentals/schedule.html"><strong aria-hidden="true">1.5.</strong> Schedule</a></li><li class="chapter-item expanded "><a href="fundamentals/builder.html"><strong aria-hidden="true">1.6.</strong> EntityBuilder</a></li><li class="chapter-item expanded "><a href="fundamentals/commandbuffer.html"><strong aria-hidden="true">1.7.</strong> CommandBuffer</a></li><li class="chapter-item expanded "><a href="fundamentals/relations.html"><strong aria-hidden="true">1.8.</strong> Relations</a></li><li class="chapter-item expanded "><a href="fundamentals/metadata.html"><strong aria-hidden="true">1.9.</strong> Component metadata</a></li></ol></li><li class="chapter-item expanded "><a href="query/index.html"><strong aria-hidden="true">2.</strong> Queries</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="query/basics.html"><strong aria-hidden="true">2.1.</strong> Basics</a></li><li class="chapter-item expanded "><a href="query/filters.html"><strong aria-hidden="true">2.2.</strong> Filters</a></li><li class="chapter-item expanded "><a href="query/change_detection.html"><strong aria-hidden="true">2.3.</strong> Change Detection</a></li><li class="chapter-item expanded "><a href="query/entity_query.html"><strong aria-hidden="true">2.4.</strong> Entity Query</a></li><li class="chapter-item expanded "><a href="query/graphs.html"><strong aria-hidden="true">2.5.</strong> Graphs</a></li></ol></li><li class="chapter-item expanded "><a href="diving_deeper/index.html"><strong aria-hidden="true">3.</strong>  Diving deeper </a></li><li><ol class="section"><li class="chapter-item expanded "><a href="diving_deeper/query.html"><strong aria-hidden="true">3.1.</strong> Advanced Queries</a></li><li class="chapter-item expanded "><a href="diving_deeper/dynamic_components.html"><strong aria-hidden="true">3.2.</strong> Dynamic Components</a></li><li class="chapter-item expanded "><a href="diving_deeper/serde.html"><strong aria-hidden="true">3.3.</strong> Serialization</a></li></ol></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString();
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
