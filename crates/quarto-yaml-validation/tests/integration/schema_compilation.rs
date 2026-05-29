use quarto_yaml_validation::{Schema, SchemaRegistry};

/// Test compiling a schema with inheritance
#[test]
fn test_compile_with_inheritance() {
    let mut registry = SchemaRegistry::new();

    // Register base schema
    let base_yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    id: string
    created_at: string
  required: [id]
"#,
    )
    .unwrap();
    let base = Schema::from_yaml(&base_yaml).unwrap();
    registry.register("base".to_string(), base);

    // Parse derived schema with inheritance
    let derived_yaml = quarto_yaml::parse(
        r#"
object:
  super:
    resolveRef: base
  properties:
    name: string
    email: string
  required: [name]
"#,
    )
    .unwrap();
    let derived = Schema::from_yaml(&derived_yaml).unwrap();

    // Before compilation, derived has base_schema
    match &derived {
        Schema::Object(obj) => {
            assert!(obj.base_schema.is_some());
            assert_eq!(obj.properties.len(), 2); // Only derived props
        }
        _ => panic!("Expected Object schema"),
    }

    // Compile
    let compiled = derived.compile(&registry).unwrap();

    // After compilation, schema has merged properties
    match compiled {
        Schema::Object(obj) => {
            assert!(obj.base_schema.is_none()); // Inheritance resolved
            assert_eq!(obj.properties.len(), 4); // Base + derived props
            assert!(obj.properties.contains_key("id"));
            assert!(obj.properties.contains_key("created_at"));
            assert!(obj.properties.contains_key("name"));
            assert!(obj.properties.contains_key("email"));
            assert_eq!(obj.required.len(), 2); // [id, name]
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test compiling eager vs lazy references
#[test]
fn test_compile_eager_vs_lazy_refs() {
    let mut registry = SchemaRegistry::new();

    // Register a schema
    let target_yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    value: string
"#,
    )
    .unwrap();
    registry.register(
        "target".to_string(),
        Schema::from_yaml(&target_yaml).unwrap(),
    );

    // Schema with both eager and lazy refs
    let yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    eager_prop:
      resolveRef: target
    lazy_prop:
      ref: target
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    // Compile
    let compiled = schema.compile(&registry).unwrap();

    match compiled {
        Schema::Object(obj) => {
            // Eager ref should be resolved to actual object
            match obj.properties.get("eager_prop") {
                Some(Schema::Object(eager_obj)) => {
                    assert!(eager_obj.properties.contains_key("value"));
                }
                _ => panic!("Expected eager_prop to be resolved to Object"),
            }

            // Lazy ref should still be a ref
            match obj.properties.get("lazy_prop") {
                Some(Schema::Ref(lazy_ref)) => {
                    assert_eq!(lazy_ref.reference, "target");
                    assert!(!lazy_ref.eager);
                }
                _ => panic!("Expected lazy_prop to remain as Ref"),
            }
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test compiling nested schemas (anyOf, allOf, array)
#[test]
fn test_compile_nested_schemas() {
    let mut registry = SchemaRegistry::new();

    // Register base schemas
    let string_schema_yaml = quarto_yaml::parse("string").unwrap();
    registry.register(
        "string-schema".to_string(),
        Schema::from_yaml(&string_schema_yaml).unwrap(),
    );

    let number_schema_yaml = quarto_yaml::parse("number").unwrap();
    registry.register(
        "number-schema".to_string(),
        Schema::from_yaml(&number_schema_yaml).unwrap(),
    );

    // Schema with nested eager refs
    let yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    flexible:
      anyOf:
        - resolveRef: string-schema
        - resolveRef: number-schema
    list:
      array:
        items:
          resolveRef: string-schema
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    // Compile
    let compiled = schema.compile(&registry).unwrap();

    match compiled {
        Schema::Object(obj) => {
            // anyOf schemas should be resolved
            match obj.properties.get("flexible") {
                Some(Schema::AnyOf(anyof)) => {
                    assert_eq!(anyof.schemas.len(), 2);
                    assert!(matches!(anyof.schemas[0], Schema::String(_)));
                    assert!(matches!(anyof.schemas[1], Schema::Number(_)));
                }
                _ => panic!("Expected AnyOf schema"),
            }

            // array items should be resolved
            match obj.properties.get("list") {
                Some(Schema::Array(arr)) => {
                    assert!(arr.items.is_some());
                    assert!(matches!(
                        arr.items.as_ref().unwrap().as_ref(),
                        Schema::String(_)
                    ));
                }
                _ => panic!("Expected Array schema"),
            }
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test that primitives compile to themselves
#[test]
fn test_compile_primitives() {
    let registry = SchemaRegistry::new();

    let primitives = vec!["boolean", "string", "number", "null", "any"];

    for primitive in primitives {
        let yaml = quarto_yaml::parse(primitive).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        let compiled = schema.compile(&registry).unwrap();

        // Primitives should compile to themselves
        assert_eq!(schema, compiled);
    }
}

/// Test error: missing eager reference
#[test]
fn test_compile_error_missing_eager_ref() {
    let registry = SchemaRegistry::new(); // Empty registry

    let yaml = quarto_yaml::parse(
        r#"
object:
  super:
    resolveRef: non-existent
  properties:
    name: string
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    let result = schema.compile(&registry);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("not found in registry"));
    assert!(err_msg.contains("non-existent"));
}

/// Test that lazy refs with missing targets don't error during compilation
#[test]
fn test_compile_preserves_lazy_ref_to_missing_target() {
    let registry = SchemaRegistry::new(); // Empty registry

    let yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    person:
      ref: non-existent
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    // Should NOT error - lazy refs are not resolved during compilation
    let compiled = schema.compile(&registry).unwrap();

    match compiled {
        Schema::Object(obj) => match obj.properties.get("person") {
            Some(Schema::Ref(r)) => {
                assert_eq!(r.reference, "non-existent");
                assert!(!r.eager);
            }
            _ => panic!("Expected Ref schema"),
        },
        _ => panic!("Expected Object schema"),
    }
}

/// Test circular lazy references are preserved
#[test]
fn test_compile_circular_lazy_refs() {
    let mut registry = SchemaRegistry::new();

    // Register person schema with circular reference to itself
    let person_yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    name: string
    parent:
      ref: person
"#,
    )
    .unwrap();
    let person_schema = Schema::from_yaml(&person_yaml).unwrap();
    registry.register("person".to_string(), person_schema);

    // Compile the registered schema
    let person_from_registry = registry.resolve("person").unwrap();
    let compiled = person_from_registry.compile(&registry).unwrap();

    // Should succeed - lazy refs are not resolved
    match compiled {
        Schema::Object(obj) => {
            assert!(obj.properties.contains_key("name"));
            assert!(obj.properties.contains_key("parent"));

            // parent should still be a ref
            match obj.properties.get("parent") {
                Some(Schema::Ref(r)) => {
                    assert_eq!(r.reference, "person");
                    assert!(!r.eager);
                }
                _ => panic!("Expected Ref schema for parent"),
            }
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test multiple inheritance compilation
#[test]
fn test_compile_multiple_inheritance() {
    let mut registry = SchemaRegistry::new();

    // Register base schemas
    let base1_yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    field1: string
  required: [field1]
"#,
    )
    .unwrap();
    registry.register("base1".to_string(), Schema::from_yaml(&base1_yaml).unwrap());

    let base2_yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    field2: number
  required: [field2]
"#,
    )
    .unwrap();
    registry.register("base2".to_string(), Schema::from_yaml(&base2_yaml).unwrap());

    // Derived with multiple bases
    let derived_yaml = quarto_yaml::parse(
        r#"
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    field3: boolean
"#,
    )
    .unwrap();
    let derived = Schema::from_yaml(&derived_yaml).unwrap();

    // Compile
    let compiled = derived.compile(&registry).unwrap();

    match compiled {
        Schema::Object(obj) => {
            assert_eq!(obj.properties.len(), 3);
            assert!(obj.properties.contains_key("field1"));
            assert!(obj.properties.contains_key("field2"));
            assert!(obj.properties.contains_key("field3"));
            assert_eq!(obj.required.len(), 2);
            assert!(obj.required.contains(&"field1".to_string()));
            assert!(obj.required.contains(&"field2".to_string()));
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test compiling schema without any refs or inheritance
#[test]
fn test_compile_simple_schema() {
    let registry = SchemaRegistry::new();

    let yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    name: string
    age: number
  required: [name]
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    let compiled = schema.compile(&registry).unwrap();

    // Should be structurally identical (no refs or inheritance to resolve)
    match compiled {
        Schema::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            assert_eq!(obj.required.len(), 1);
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test compiling enum schema
#[test]
fn test_compile_enum() {
    let registry = SchemaRegistry::new();

    let yaml = quarto_yaml::parse(
        r#"
enum: [option1, option2, option3]
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    let compiled = schema.compile(&registry).unwrap();

    // Enums should compile to themselves
    assert_eq!(schema, compiled);
}
