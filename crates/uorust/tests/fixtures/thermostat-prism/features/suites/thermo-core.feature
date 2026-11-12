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
  Scenario: thermo_core::band: Temperature bands, counters and readings.
    Given the record Interface::featureThermoCoreBand
    When the interface call Interface::callThermoCoreBandWidth is made
    And the interface call Interface::callThermoCoreBandCheckedMidpoint is made
    And the interface call Interface::callThermoCoreBandTick is made
    And the interface call Interface::callThermoCoreBandShortfall is made
    And the interface call Interface::callThermoCoreBandFreezing is made
    And the interface call Interface::callThermoCoreBandDelta is made
    And the interface call Interface::callThermoCoreBandChecked is made
    And the interface call Interface::callThermoCoreBandIsCelsius is made
    And the interface call Interface::callThermoCoreBandAbove is made
    And the interface call Interface::callThermoCoreBandFirstReading is made
    And the interface call Interface::callThermoCoreBandTargetOr is made
    And the interface call Interface::callThermoCoreBandHeatFor is made
    And the interface call Interface::callThermoCoreBandFanLabel is made
    And the interface call Interface::callThermoCoreBandLabelled is made
    And the interface call Interface::callThermoCoreBandLowOf is made
    And the interface call Interface::callThermoCoreBandNotANumber is made
    And the interface call Interface::callThermoCoreBandWithHigh is made
    And the interface call Interface::callThermoCoreBandWithLow is made
    And the interface call Interface::callThermoCoreBandJoined is made
    And the interface call Interface::callThermoCoreBandLabelOf is made
    And the interface call Interface::callThermoCoreBandClamped is made
    Then the acceptance row Interface::acThermoCoreThermoCoreBandBandsMeasureAndCount holds
    And the acceptance row Interface::acThermoCoreThermoCoreBandCountersAndSignedReadings holds

  @FT-03 @build
  Scenario: thermo_core::config: Reads the target temperature from a file or the environment.
    Given the record Interface::featureThermoCoreConfig
    When the interface call Interface::callThermoCoreConfigParseTarget is made
    And the interface call Interface::callThermoCoreConfigLoadTarget is made
    And the interface call Interface::callThermoCoreConfigTargetFromEnv is made
    Then the acceptance row Interface::acThermoCoreConfigParsesAWholeNumber holds

  @FT-04 @build
  Scenario: thermo_core::roster: Rooms, their readings and the schedule that names them.
    Given the record Interface::featureThermoCoreRoster
    When the interface call Interface::callThermoCoreRosterRoomIsCold is made
    And the interface call Interface::callThermoCoreRosterRoomReread is made
    And the interface call Interface::callThermoCoreRosterRoomLabel is made
    And the interface call Interface::callThermoCoreRosterNames is made
    And the interface call Interface::callThermoCoreRosterCold is made
    And the interface call Interface::callThermoCoreRosterAnyCold is made
    And the interface call Interface::callThermoCoreRosterAllWithin is made
    And the interface call Interface::callThermoCoreRosterFirstCold is made
    And the interface call Interface::callThermoCoreRosterHighest is made
    And the interface call Interface::callThermoCoreRosterColdCount is made
    And the interface call Interface::callThermoCoreRosterScheduled is made
    And the interface call Interface::callThermoCoreRosterRoll is made
    And the interface call Interface::callThermoCoreRosterPairs is made
    And the interface call Interface::callThermoCoreRosterTargetOf is made
    And the interface call Interface::callThermoCoreRosterRoomsOf is made
    And the interface call Interface::callThermoCoreRosterHasTarget is made
    And the interface call Interface::callThermoCoreRosterClosed is made
    And the interface call Interface::callThermoCoreRosterLabelWidth is made
    And the interface call Interface::callThermoCoreRosterClamp is made
    And the interface call Interface::callThermoCoreRosterBlank is made
    And the interface call Interface::callThermoCoreRosterColdNames is made
    And the interface call Interface::callThermoCoreRosterReport is made
    And the interface call Interface::callThermoCoreRosterReadingsOf is made
    And the interface call Interface::callThermoCoreRosterLineRoom is made
    And the interface call Interface::callThermoCoreRosterLineTarget is made
    And the interface call Interface::callThermoCoreRosterScheduleRooms is made
    And the interface call Interface::callThermoCoreRosterRoomsUntilStop is made
    And the interface call Interface::callThermoCoreRosterTag is made
    And the interface call Interface::callThermoCoreRosterIsZone is made
    And the interface call Interface::callThermoCoreRosterDivider is made
    And the interface call Interface::callThermoCoreRosterSignKind is made
    And the interface call Interface::callThermoCoreRosterDigitCount is made
    And the interface call Interface::callThermoCoreRosterNumbered is made
    And the interface call Interface::callThermoCoreRosterPositions is made
    And the interface call Interface::callThermoCoreRosterFirstIs is made
    And the interface call Interface::callThermoCoreRosterFirstWord is made
    And the interface call Interface::callThermoCoreRosterSpelled is made
    And the interface call Interface::callThermoCoreRosterSlots is made
    And the interface call Interface::callThermoCoreRosterZoneHeats is made
    And the interface call Interface::callThermoCoreRosterAllNamed is made
    And the interface call Interface::callThermoCoreRosterZoneRooms is made
    And the interface call Interface::callThermoCoreRosterApart is made
    And the interface call Interface::callThermoCoreRosterRequestedRooms is made
    And the interface call Interface::callThermoCoreRosterOwnRooms is made
    And the interface call Interface::callThermoCoreRosterAllGroupsNamed is made
    And the interface call Interface::callThermoCoreRosterTrimmed is made
    And the interface call Interface::callThermoCoreRosterUrgentRooms is made
    And the interface call Interface::callThermoCoreRosterBandWidths is made
    And the interface call Interface::callThermoCoreRosterSaturatedTotal is made
    And the interface call Interface::callThermoCoreRosterPaired is made
    And the interface call Interface::callThermoCoreRosterUrgentAndNamed is made
    And the interface call Interface::callThermoCoreRosterSecondLast is made
    And the interface call Interface::callThermoCoreRosterAboveTen is made
    And the interface call Interface::callThermoCoreRosterWarm is made
    And the interface call Interface::callThermoCoreRosterBooked is made
    And the interface call Interface::callThermoCoreRosterPerRoom is made
    And the interface call Interface::callThermoCoreRosterLeftOver is made
    And the interface call Interface::callThermoCoreRosterUps is made
    And the interface call Interface::callThermoCoreRosterScheduleFile is made
    And the interface call Interface::callThermoCoreRosterMissingSchedule is made
    And the interface call Interface::callThermoCoreRosterNoneTaken is made
    And the interface call Interface::callThermoCoreRosterAllBooked is made
    And the interface call Interface::callThermoCoreRosterRosterChecks is made
    And the interface call Interface::callThermoCoreRosterFiledName is made
    And the interface call Interface::callThermoCoreRosterSameFile is made
    And the interface call Interface::callThermoCoreRosterCutName is made
    And the interface call Interface::callThermoCoreRosterCountByte is made
    And the interface call Interface::callThermoCoreRosterCodeIndex is made
    And the interface call Interface::callThermoCoreRosterInComfortBand is made
    And the interface call Interface::callThermoCoreRosterPressing is made
    And the interface call Interface::callThermoCoreRosterFloors is made
    And the interface call Interface::callThermoCoreRosterRoomLabels is made
    And the interface call Interface::callThermoCoreRosterReadingWithin is made
    And the interface call Interface::callThermoCoreRosterBookedOrFirst is made
    And the interface call Interface::callThermoCoreRosterNamedOrSpare is made
    And the interface call Interface::callThermoCoreRosterStartsUnnamed is made
    And the interface call Interface::callThermoCoreRosterMarkedRooms is made
    And the interface call Interface::callThermoCoreRosterDoubledRoom is made
    And the interface call Interface::callThermoCoreRosterQueue is made
    And the interface call Interface::callThermoCoreRosterSortsBefore is made
    And the interface call Interface::callThermoCoreRosterSameZone is made
    And the interface call Interface::callThermoCoreRosterSetPoint is made
    And the interface call Interface::callThermoCoreRosterFirst is made
    And the interface call Interface::callThermoCoreRosterOpening is made
    And the interface call Interface::callThermoCoreRosterSpareRoom is made
    And the interface call Interface::callThermoCoreRosterEveryOther is made
    And the interface call Interface::callThermoCoreRosterFirstRooms is made
    And the interface call Interface::callThermoCoreRosterAnnexedRoom is made
    And the interface call Interface::callThermoCoreRosterRenamed is made
    And the interface call Interface::callThermoCoreRosterPrefixed is made
    And the interface call Interface::callThermoCoreRosterByReading is made
    And the interface call Interface::callThermoCoreRosterOpeningBand is made
    Then the acceptance row Interface::acThermoCoreThermoCoreRosterRequestsSortByPriorityThenRoom holds
    And the acceptance row Interface::acThermoCoreThermoCoreRosterTheRosterFoldsAndClamps holds
    And the acceptance row Interface::acThermoCoreThermoCoreRosterTheRosterLoopsOverItsRooms holds
    And the acceptance row Interface::acThermoCoreThermoCoreRosterTheRosterReadsItsMaps holds
    And the acceptance row Interface::acThermoCoreThermoCoreRosterTheRosterReadsItsRooms holds
    And the acceptance row Interface::acThermoCoreThermoCoreRosterTheRosterReadsText holds

  @FT-05 @build
  Scenario: thermo_core::schedule: A heating schedule: steps that hold a target, choose between steps, or run labelled phases in order.
    Given the record Interface::featureThermoCoreSchedule
    When the interface call Interface::callThermoCoreScheduleOccupiedTarget is made
    And the interface call Interface::callThermoCoreScheduleIsSequence is made
    And the interface call Interface::callThermoCoreScheduleHold is made
    And the interface call Interface::callThermoCoreSchedulePhaseLabel is made
    And the interface call Interface::callThermoCoreScheduleIsFinal is made
    And the interface call Interface::callThermoCoreScheduleLabelOr is made
    And the interface call Interface::callThermoCoreScheduleLabels is made
    And the interface call Interface::callThermoCoreScheduleHeldTargets is made
    And the interface call Interface::callThermoCoreScheduleDescribe is made
    And the interface call Interface::callThermoCoreScheduleValidate is made
    And the interface call Interface::callThermoCoreScheduleFirstHold is made
    And the interface call Interface::callThermoCoreScheduleSingleLabel is made
    And the interface call Interface::callThermoCoreScheduleModeFor is made
    And the interface call Interface::callThermoCoreScheduleComfortable is made
    And the interface call Interface::callThermoCoreScheduleEither is made
    And the interface call Interface::callThermoCoreScheduleHoldsOff is made
    And the interface call Interface::callThermoCoreSchedulePhasesOff is made
    And the interface call Interface::callThermoCoreSchedulePhaseOff is made
    Then the acceptance row Interface::acThermoCoreScheduleAHeldPhaseKeepsItsLabel holds
    And the acceptance row Interface::acThermoCoreScheduleLabelsAreKeptOnceAndInOrder holds
    And the acceptance row Interface::acThermoCoreThermoCoreScheduleNestedPatternsChooseTheArm holds

  @FT-06 @build
  Scenario: thermo_core::sensor: Temperature sensors the controller reads from.
    Given the record Interface::featureThermoCoreSensor
    When the interface call Interface::callThermoCoreSensorSensorRead is made
    And the interface call Interface::callThermoCoreSensorSensorCalibrated is made
    And the interface call Interface::callThermoCoreSensorTotal is made
    And the interface call Interface::callThermoCoreSensorAtLeast is made
    Then the acceptance row Interface::acThermoCoreThermoCoreSensorAFixedSensorReadsItsValue holds
