#!/bin/bash

# Takes a screenshot, uploads it using curl, and opens the public URL in the default browser.
# Add -u user:pass to curl if you use authentication for uploads, or modify your .netrc and add -n.

scrot -s -e $'curl -s -F file=@$f -F options=\'{"useOriginalFilename":false}\' https://i.zozs.se | jq -r .url | xargs xdg-open; rm $f'
