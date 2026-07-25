"""Constants for the Smart Alarm Clock integration."""

DOMAIN = "smart_alarm_clock"
DEFAULT_NAME = "Smart Alarm Clock"

# The device always serves its REST API on :80 (the api client's default) and
# the realtime SSE stream on :81.
SSE_PORT = 81
