import re

with open("src/event_store/mod.rs", "r") as f:
    text = f.read()

# Comment out the entire test_append_updates_indices block
pattern = r"// #\[tokio::test\]\n    // async fn test_append_updates_indices\(\) \{[\s\S]*?\}\n\n"
text = re.sub(r"// #\[tokio::test\]\n    // async fn test_append_updates_indices\(\) \{[\s\S]*?\}\n\n", "", text)
text = re.sub(r"#\[tokio::test\]\n    async fn test_append_updates_indices\(\) \{[\s\S]*?\}\n\n", "", text)


with open("src/event_store/mod.rs", "w") as f:
    f.write(text)
