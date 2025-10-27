use quarto_yaml_validation::{Schema, SchemaRegistry, merge_object_schemas};
use quarto_yaml;

/// Test based on real quarto-cli definitions.yml schema
/// social-metadata → twitter-card-config inheritance
#[test]
fn test_twitter_card_inheritance() {
    let mut registry = SchemaRegistry::new();

    // Register base schema (social-metadata)
    let base_yaml = quarto_yaml::parse(r#"
object:
  properties:
    title:
      string:
        description: "Title for social media"
    description:
      string:
        description: "Description for social media"
    image:
      string:
        description: "Image URL"
  required: [title]
"#).unwrap();

    let base_schema = Schema::from_yaml(&base_yaml).unwrap();
    registry.register("social-metadata".to_string(), base_schema);

    // Parse derived schema (twitter-card-config)
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: social-metadata
  closed: true
  properties:
    card-style:
      enum: [summary, summary_large_image]
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    // Extract base_schema and merge
    match derived_schema {
        Schema::Object(ref obj) => {
            assert!(obj.base_schema.is_some());

            let merged = merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            // Verify merged schema has properties from both
            assert!(merged.properties.contains_key("title"));
            assert!(merged.properties.contains_key("description"));
            assert!(merged.properties.contains_key("image"));
            assert!(merged.properties.contains_key("card-style"));

            // Verify required from base
            assert!(merged.required.contains(&"title".to_string()));

            // Verify closed from derived
            assert!(merged.closed);
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test multiple inheritance
#[test]
fn test_multiple_inheritance() {
    let mut registry = SchemaRegistry::new();

    // Register base1
    let base1_yaml = quarto_yaml::parse(r#"
object:
  properties:
    field1: string
  required: [field1]
"#).unwrap();
    registry.register("base1".to_string(), Schema::from_yaml(&base1_yaml).unwrap());

    // Register base2
    let base2_yaml = quarto_yaml::parse(r#"
object:
  properties:
    field2: number
  required: [field2]
"#).unwrap();
    registry.register("base2".to_string(), Schema::from_yaml(&base2_yaml).unwrap());

    // Parse derived with multiple bases
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    field3: boolean
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    match derived_schema {
        Schema::Object(ref obj) => {
            let merged = merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            assert_eq!(merged.properties.len(), 3);
            assert!(merged.properties.contains_key("field1"));
            assert!(merged.properties.contains_key("field2"));
            assert!(merged.properties.contains_key("field3"));

            assert_eq!(merged.required.len(), 2);
            assert!(merged.required.contains(&"field1".to_string()));
            assert!(merged.required.contains(&"field2".to_string()));
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test property override
#[test]
fn test_property_override() {
    let mut registry = SchemaRegistry::new();

    // Base has 'name' as string
    let base_yaml = quarto_yaml::parse(r#"
object:
  properties:
    name:
      string:
        description: "Base description"
"#).unwrap();
    registry.register("base".to_string(), Schema::from_yaml(&base_yaml).unwrap());

    // Derived overrides 'name' with different constraints
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: base
  properties:
    name:
      string:
        pattern: "^[A-Z]"
        description: "Derived description"
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    match derived_schema {
        Schema::Object(ref obj) => {
            let merged = merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            // Derived should win
            match merged.properties.get("name") {
                Some(Schema::String(s)) => {
                    assert_eq!(s.pattern, Some("^[A-Z]".to_string()));
                    assert_eq!(s.annotations.description, Some("Derived description".to_string()));
                }
                _ => panic!("Expected string schema for name"),
            }
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test that base schema without inheritance works
#[test]
fn test_no_inheritance() {
    let _registry = SchemaRegistry::new();

    let yaml = quarto_yaml::parse(r#"
object:
  properties:
    name: string
    age: number
  required: [name]
"#).unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();

    match schema {
        Schema::Object(ref obj) => {
            // No base_schema, nothing to merge
            assert!(obj.base_schema.is_none());
            assert_eq!(obj.properties.len(), 2);
            assert_eq!(obj.required.len(), 1);
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test inline object as super (not just ref)
#[test]
fn test_inline_super() {
    let registry = SchemaRegistry::new();

    let yaml = quarto_yaml::parse(r#"
object:
  super:
    object:
      properties:
        base_prop: string
      required: [base_prop]
  properties:
    derived_prop: number
"#).unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();

    match schema {
        Schema::Object(ref obj) => {
            assert!(obj.base_schema.is_some());

            let merged = merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            assert!(merged.properties.contains_key("base_prop"));
            assert!(merged.properties.contains_key("derived_prop"));
            assert!(merged.required.contains(&"base_prop".to_string()));
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test that required: "all" works with inheritance
#[test]
fn test_required_all_with_inheritance() {
    let mut registry = SchemaRegistry::new();

    let base_yaml = quarto_yaml::parse(r#"
object:
  properties:
    id: string
    name: string
  required: [id]
"#).unwrap();
    registry.register("base".to_string(), Schema::from_yaml(&base_yaml).unwrap());

    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: base
  properties:
    email: string
    phone: string
  required: all
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    match derived_schema {
        Schema::Object(ref obj) => {
            // required: all should expand to [email, phone] for derived props only
            assert_eq!(obj.required.len(), 2);
            assert!(obj.required.contains(&"email".to_string()));
            assert!(obj.required.contains(&"phone".to_string()));

            let merged = merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            // After merge, should have all three required fields
            assert_eq!(merged.required.len(), 3);
            assert!(merged.required.contains(&"id".to_string()));
            assert!(merged.required.contains(&"email".to_string()));
            assert!(merged.required.contains(&"phone".to_string()));
        }
        _ => panic!("Expected Object schema"),
    }
}
