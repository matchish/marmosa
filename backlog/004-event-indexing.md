# Event Indexing Engine

## Status: Not Started

## Description
Currently `read_internal` scans all events linearly. Add tag-based indices to enable O(log N) lookups instead of O(N) scans.

## Acceptance Criteria
- [ ] Tag indices stored at `Indices/{tag_key}/{tag_value}.json`
- [ ] Each index file contains list of event positions
- [ ] Indices updated on event append
- [ ] `read_async` uses indices when query has tags
- [ ] Falls back to linear scan for complex queries

## Storage Format
```
Events/
  0000000001.json
  0000000002.json
Indices/
  course_id/
    course-123.json  -> [1, 5, 12]
    course-456.json  -> [2, 3, 8]
  student_id/
    student-001.json -> [4, 7, 9]
```

## Performance Target
- Query by single tag: O(index_size) instead of O(total_events)
- Query by multiple tags: intersection of indices

## Dependencies
- EventStore append_async (done)
- EventStore read_async (done)
