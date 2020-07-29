#!/bin/bash

# Takes a screenshot, uploads it using curl, and places the resulting public URL in the clipboard.
# Also shows a notification when upload is finished.
# Add -u admin:password to curl if you use authentication for uploads.

scrot -s -e $'curl -s -F file=@$f -F options=\'{"useOriginalFilename":false}\' http://localhost:8088 | jq -j .url | xclip; rm $f' && notify-send "Upload finished" "Screenshot uploaded successfully"
