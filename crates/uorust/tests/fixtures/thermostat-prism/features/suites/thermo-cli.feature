Feature: thermo-cli

  The features uorust admitted for the thermo-cli package, each with the
  interface calls that realize it and the acceptance rows that accept it.

  @FT-05 @build
  Scenario: thermo: Prints whether to heat, for a reading given on the command line.
    Given the record Interface::featureThermoMain
    When the interface call Interface::callCliThermoMain is made
    Then the acceptance row Interface::acThermoCliCliColdReadingHeats holds
