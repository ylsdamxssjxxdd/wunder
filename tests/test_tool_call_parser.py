from app.orchestrator.tool_executor import ToolCallParser


def test_tool_call_parser_tool_call_tag():
    text = '<tool_call>{"name":"foo","arguments":{"a":1}}</tool_call>'
    calls = ToolCallParser.parse(text)
    assert len(calls) == 1
    assert calls[0]["name"] == "foo"
    assert calls[0]["arguments"]["a"] == 1


def test_tool_call_parser_tool_tag():
    text = '<tool>{"name":"bar","arguments":{"b":2}}</tool>'
    calls = ToolCallParser.parse(text)
    assert len(calls) == 1
    assert calls[0]["name"] == "bar"
    assert calls[0]["arguments"]["b"] == 2


def test_tool_call_parser_unclosed_tag_with_trailing_text():
    text = '<tool_call>{"name":"baz","arguments":{"c":3}} trailing'
    calls = ToolCallParser.parse(text)
    assert len(calls) == 1
    assert calls[0]["name"] == "baz"
    assert calls[0]["arguments"]["c"] == 3


def test_tool_call_parser_arguments_as_json_string():
    text = '<tool>{"name":"foo","arguments":"{\\"a\\":1}"}</tool>'
    calls = ToolCallParser.parse(text)
    assert len(calls) == 1
    assert calls[0]["arguments"]["a"] == 1


def test_tool_call_parser_list_payload():
    text = (
        '<tool_call>[{"name":"one","arguments":{"a":1}},'
        '{"name":"two","arguments":{"b":2}}]</tool_call>'
    )
    calls = ToolCallParser.parse(text)
    assert len(calls) == 2
    assert calls[0]["name"] == "one"
    assert calls[1]["name"] == "two"
