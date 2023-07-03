#!/bin/bash

# Takes a screenshot of part of screen using slurp + grim, suitable for Wayland.
# Then uploads it using curl, and opens the public URL in the default browser.
# Add -u user:pass to curl if you use authentication for uploads, or modify your .netrc and add -n.

slurp | grim -g - - | curl -n -s -F "file=@-;filename=temp.png" http://localhost:8088 | jq -r .url | xargs xdg-open
