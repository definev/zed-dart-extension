; Dart main
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import)))))
 ((function_signature
   name: (identifier) @run)
  (#eq? @run "main"))
 (#match? @_import "dart:")
 (#set! tag dart-main))

; Flutter main
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import)))))
 ((function_signature
   name: (identifier) @run)
  (#eq? @run "main"))
 (#match? @_import "package:flutter/(material|widgets|cupertino).dart")
 (#not-match? @_import "package:flutter_test/flutter_test.dart")
 (#not-match? @_import "package:test/test.dart")
 (#set! tag flutter-main))

; Dart test file
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @run
          (#match? @run "package:test/test.dart"))))))
 (#set! tag dart-test-file))

; Flutter test file
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @run
          (#match? @run "package:flutter_test/flutter_test.dart"))))))
 (#set! tag flutter-test-file))

; Dart test group
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import
          (#match? @_import "package:test/test.dart"))))))
 (function_body
   (block
     (expression_statement
       ((identifier) @run
        (#eq? @run "group")))))
 (#set! tag dart-test-group))

; Flutter test group
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import
          (#match? @_import "package:flutter_test/flutter_test.dart"))))))
 (function_body
   (block
     (expression_statement
       ((identifier) @run
        (#eq? @run "group")))))
 (#set! tag flutter-test-group))

; Dart test single
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import
          (#match? @_import "package:test/test.dart"))))))
 (function_body
   (block
     (expression_statement
       (selector
         (argument_part
           (arguments
             (argument
               (function_expression
                 (function_expression_body
                   (block
                     (expression_statement
                       ((identifier) @run
                        (#eq? @run "test"))))))))))))
 (#set! tag dart-test-single)))

; Flutter test single
((import_or_export
  (library_import
    (import_specification
      (configurable_uri
        (uri
          (string_literal) @_import
          (#match? @_import "package:flutter_test/flutter_test.dart"))))))
 (function_body
   (block
     (expression_statement
       (selector
         (argument_part
           (arguments
             (argument
               (function_expression
                 (function_expression_body
                   (block
                     (expression_statement
                       ((identifier) @run
                        (#match? @run "^(test|testWidgets)$"))))))))))))
 (#set! tag flutter-test-single)))
