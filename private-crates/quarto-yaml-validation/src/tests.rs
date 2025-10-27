// Tests for YAML validation

#[cfg(test)]
mod integration_tests {
    use crate::schema::*;
    use crate::validator::validate;
    use quarto_source_map::SourceContext;
    use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
    use yaml_rust2::Yaml;

    fn make_yaml_bool(value: bool) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar(Yaml::Boolean(value), SourceInfo::default())
    }

    fn make_yaml_string(value: &str) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar(Yaml::String(value.to_string()), SourceInfo::default())
    }

    fn make_yaml_number(value: i64) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar(Yaml::Integer(value), SourceInfo::default())
    }

    fn make_source_ctx() -> SourceContext {
        SourceContext::new()
    }

    #[test]
    fn test_boolean_validation() {
        let registry = SchemaRegistry::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = make_yaml_bool(true);
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_ok());

        let yaml = make_yaml_string("not a boolean");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());
    }

    #[test]
    fn test_string_validation() {
        let registry = SchemaRegistry::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: Some(3),
            max_length: Some(10),
            pattern: None,
        });

        let yaml = make_yaml_string("hello");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_ok());

        let yaml = make_yaml_string("hi");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());

        let yaml = make_yaml_string("this is too long");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());
    }

    #[test]
    fn test_number_validation() {
        let registry = SchemaRegistry::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: Some(0.0),
            maximum: Some(100.0),
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        let yaml = make_yaml_number(50);
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_ok());

        let yaml = make_yaml_number(-1);
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());

        let yaml = make_yaml_number(101);
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());
    }

    #[test]
    fn test_enum_validation() {
        let registry = SchemaRegistry::new();
        let schema = Schema::Enum(EnumSchema {
            annotations: SchemaAnnotations::default(),
            values: vec![
                serde_json::Value::String("red".to_string()),
                serde_json::Value::String("green".to_string()),
                serde_json::Value::String("blue".to_string()),
            ],
        });

        let yaml = make_yaml_string("red");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_ok());

        let yaml = make_yaml_string("yellow");
        assert!(validate(&yaml, &schema, &registry, &make_source_ctx()).is_err());
    }

    #[test]
    fn test_schema_true_and_false() {
        let registry = SchemaRegistry::new();
        let yaml = make_yaml_bool(true);

        let schema_true = Schema::True;
        assert!(validate(&yaml, &schema_true, &registry, &make_source_ctx()).is_ok());

        let schema_false = Schema::False;
        assert!(validate(&yaml, &schema_false, &registry, &make_source_ctx()).is_err());
    }
}
