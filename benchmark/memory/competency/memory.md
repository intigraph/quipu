# Competency questions: Memory metadata

These questions declare the tested merge policy. They do not establish a
universal schema for Memories. Cardinality and datatype declarations are in
model.py; question 6 motivates retaining provenance as part of the version.

## Questions

1. What is the current status of `<memory>` at this version?
2. Until what timezone-qualified date and time is `<memory>` valid at this version?
3. Which evidence strings support `<memory>`?
4. Which targets does `<memory>` apply to, and which Links connect those targets?
5. Which tags categorize `<memory>`?
6. Which version set each metadata fact about `<memory>`?
7. Which Memories currently have active status?
8. Which Memories carry the selected tag?
9. Which active Memories apply to a selected target through a Link?

## Acceptance

Questions 1 and 2 require at most one current answer. Questions 3 through 5 allow
multiple answers. Questions 7 through 9 execute as the status, tag and traversal
SQL probes in run.py against independently constructed expected results. Question
6 makes same-value/different-provenance conflicts visible; it does not resolve them.
