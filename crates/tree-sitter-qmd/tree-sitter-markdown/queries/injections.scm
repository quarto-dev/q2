(fenced_code_block
  (info_string
    (language) @injection.language)
  (code_fence_content) @injection.content)

(fenced_code_block
  (language_attribute
    (language) @injection.language)
  (code_fence_content) @injection.content)

((metadata) @injection.content (#set! injection.language "yaml"))
