Feature: self-model

  UORust's own Prism model, in app/: its application is the route table that
  decides which Prism record each kind of Rust fact becomes, and its facets
  describe UORust itself.

  @SM-01 @build
  Scenario: UORust's application answers the route table of model/routes.toml route for route, and replays every router and workbench vector of that file in order
    Given the route table in model/routes.toml and the model in app/src/Routes.lex.tex
    When the suite exercises SM-01
    Then every route of the file is a row of the model's table and of routeOf, in order
    And the application's acceptance vectors are the file's router vectors then its workbench vectors

  @SM-02 @build
  Scenario: The route table's theorems state, over the whole table and by decide, that every fact kind routes to its own row, that facts are distinct, and that any two kinds naming different routes are refused as ambiguous
    Given the route table model in app/src/Routes.lex.tex
    When the Lean kernel checks its theorems during prismpm verify
    Then routeTableIsTotal, routeFactsAreDistinct and routeTableRefusesAmbiguity quantify over the whole table
    And each is decided, not assumed

  @SM-03 @build
  Scenario: UORust is modeled in every facet: architecture, design views, features and journeys, security with its threat model, quality, the browser system release and compliance, each with records, and every module title has a project-local glossary entry
    Given the modules in app/src and the glossaries in app/language
    When the suite exercises SM-03
    Then each facet declares records of its kinds
    And every module title is the surface of a glossary entry

  @SM-04 @build
  Scenario: The threat model maps every threat to a control with evidence or accepts it with a rationale, and every control's evidence names a gate, proof or review
    Given the threat model in app/src/Security.lex.tex
    When the suite exercises SM-04
    Then every threat has a mapping or an accepted risk
    And every control has a modeled row with at least one piece of evidence

  @SM-05 @build
  Scenario: UORust's brand identity and design system are read from uor.foundation: every Wheeler brand element and every W3C design token of the pinned site, each with its source span, and every text and focus colour the site sets on a surface meets its WCAG 2.2 contrast criterion
    Given uor.foundation as brand/foundation.lock pins it and the model in app/src/Brand.lex.tex
    When the suite exercises SM-05
    Then every brand element kind and every --fd- token of the site is a sourced record
    And every contrast's ratio, recomputed from the token colours, meets its WCAG 2.2 criterion

  @SM-06 @build
  Scenario: UORust's context of use (ISO 9241-11) is read from its own model: every journey's actor is a user group, every journey a goal whose tasks are its steps in order and each accepted, every user-facing component a technical environment, and every goal's usability measured or recorded open
    Given the journeys, features and components of app/src and the model in app/src/Users.lex.tex
    When the suite exercises SM-06
    Then every journey is a goal of its actor's user group, with its steps as tasks in order
    And every goal has an effectiveness, an efficiency and a satisfaction measure
