Feature: thermo-core

  The features uorust admitted for the thermo-core package, each with the
  interface calls that realize it and the acceptance rows that accept it.

  @FT-01 @build
  Scenario: thermo_core: Thermostat control: decides from a reading and a target whether to heat.
    Given the record Interface::featureThermoCore
    When the interface call Interface::callThermoCoreShouldHeat is made
    And the interface call Interface::callThermoCoreWithinBand is made
    And the interface call Interface::callThermoCoreNextMode is made
    And the interface call Interface::callThermoCoreHalt is made
    And the interface call Interface::callThermoCoreMidpoint is made
    Then the acceptance row Interface::acDocThermoCoreShouldHeat holds
    And the acceptance row Interface::acThermoCoreThermoCoreBandIsInclusive holds
    And the acceptance row Interface::acThermoCoreThermoCoreHeatsWhenCold holds
    And the acceptance row Interface::acThermoCoreThermoCoreIdleRoomThatIsColdHeats holds
    And the acceptance row Interface::acThermoCoreThermoCoreMidpointOfABand holds

  @FT-02 @build
  Scenario: thermo_core::config: Reads the target temperature from a file or the environment.
    Given the record Interface::featureThermoCoreConfig
    When the interface call Interface::callThermoCoreConfigParseTarget is made
    And the interface call Interface::callThermoCoreConfigLoadTarget is made
    And the interface call Interface::callThermoCoreConfigTargetFromEnv is made
    Then the acceptance row Interface::acThermoCoreConfigParsesAWholeNumber holds
