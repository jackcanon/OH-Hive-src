#!/usr/bin/env python3
"""Unit tests for cloud_card.py"""
import argparse
import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch

import cloud_card


class TestComputeVerdict(unittest.TestCase):
    """Test the verdict logic for all four combinations of met/not-met x should-fail/not."""
    
    def test_expectations_met_normal_case(self):
        """When expectations are met and should_fail is False, should pass."""
        passed, note = cloud_card.compute_verdict(expectations_met=True, should_fail=False)
        self.assertTrue(passed)
        self.assertEqual(note, "card produced what it claimed")
    
    def test_expectations_not_met_normal_case(self):
        """When expectations are not met and should_fail is False, should fail."""
        passed, note = cloud_card.compute_verdict(expectations_met=False, should_fail=False)
        self.assertFalse(passed)
        self.assertEqual(note, "card claimed more than it produced")
    
    def test_expectations_met_should_fail(self):
        """When expectations are met but should_fail is True, should fail (card succeeded when it shouldn't)."""
        passed, note = cloud_card.compute_verdict(expectations_met=True, should_fail=True)
        self.assertFalse(passed)
        self.assertEqual(note, "card SUCCEEDED but was expected to fall short")
    
    def test_expectations_not_met_should_fail(self):
        """When expectations are not met and should_fail is True, should pass (negative case working as expected)."""
        passed, note = cloud_card.compute_verdict(expectations_met=False, should_fail=True)
        self.assertTrue(passed)
        self.assertEqual(note, "card fell short, as the negative case expects")


class TestNodeKeyParsing(unittest.TestCase):
    """Test node key parsing from various file formats."""
    
    def test_explicit_key_takes_precedence(self):
        """If an explicit key is passed, it should be returned immediately."""
        result = cloud_card.node_key(explicit="explicit-key-123")
        self.assertEqual(result, "explicit-key-123")
    
    def test_environment_variable(self):
        """If HIVE_NODE_KEY is in environment, it should be used."""
        with patch.dict('os.environ', {'HIVE_NODE_KEY': 'env-key-456'}):
            result = cloud_card.node_key(explicit=None)
            self.assertEqual(result, "env-key-456")
    
    def test_export_prefix(self):
        """Should parse 'export HIVE_NODE_KEY=...' format."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("export HIVE_NODE_KEY=test-key-export\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "test-key-export")
    
    def test_single_quoted_value(self):
        """Should strip single quotes from the key value."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("HIVE_NODE_KEY='single-quoted-key'\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "single-quoted-key")
    
    def test_double_quoted_value(self):
        """Should strip double quotes from the key value."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text('HIVE_NODE_KEY="double-quoted-key"\n')
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "double-quoted-key")
    
    def test_no_quotes(self):
        """Should handle unquoted key values."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("HIVE_NODE_KEY=unquoted-key\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "unquoted-key")
    
    def test_file_with_no_hive_node_key_line(self):
        """When node.env exists but has no HIVE_NODE_KEY line, should return None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("# Some config file\nOTHER_VAR=value\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertIsNone(result)
    
    def test_nonexistent_file(self):
        """When no node.env file exists, should return None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            nonexistent = Path(tmpdir) / "nonexistent" / "node.env"
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [nonexistent]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertIsNone(result)
    
    def test_export_with_quotes(self):
        """Should handle 'export HIVE_NODE_KEY="..."' format."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text('export HIVE_NODE_KEY="exported-quoted-key"\n')
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "exported-quoted-key")
    
    def test_multiple_lines_with_comments(self):
        """Should find HIVE_NODE_KEY in a file with multiple lines and comments."""
        with tempfile.TemporaryDirectory() as tmpdir:
            node_env = Path(tmpdir) / "node.env"
            node_env.write_text("""# Node configuration
# Generated by hive
OTHER_SETTING=foo
HIVE_NODE_KEY=multi-line-key
ANOTHER_SETTING=bar
""")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [node_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "multi-line-key")
    
    def test_first_candidate_takes_precedence(self):
        """When multiple candidate files exist, the first one should be used."""
        with tempfile.TemporaryDirectory() as tmpdir:
            first_env = Path(tmpdir) / "first.env"
            first_env.write_text("HIVE_NODE_KEY=first-key\n")
            
            second_env = Path(tmpdir) / "second.env"
            second_env.write_text("HIVE_NODE_KEY=second-key\n")
            
            with patch.object(cloud_card, 'NODE_ENV_CANDIDATES', [first_env, second_env]):
                with patch.dict('os.environ', {}, clear=True):
                    result = cloud_card.node_key(explicit=None)
                    self.assertEqual(result, "first-key")


class TestAcceptanceCheckParsing(unittest.TestCase):
    """--check / --check-advisory / --check-json, added when the acceptance gate got a submit path.

    The shorthand is whitespace-split on purpose: the host runs the program directly with no shell,
    so pretending to accept a shell command line here would produce a check that fails on the node
    for a reason the caller could not see from what they typed."""

    def test_program_only(self):
        self.assertEqual(cloud_card.parse_check("build=make"),
                         {"name": "build", "command": "make"})

    def test_program_with_args(self):
        self.assertEqual(cloud_card.parse_check("tests=cargo test --quiet"),
                         {"name": "tests", "command": "cargo", "args": ["test", "--quiet"]})

    def test_advisory_sets_required_false(self):
        """Required is the host's default, so it is only emitted when it is being turned off."""
        self.assertEqual(cloud_card.parse_check("fmt=cargo fmt", required=False),
                         {"name": "fmt", "command": "cargo", "args": ["fmt"], "required": False})
        self.assertNotIn("required", cloud_card.parse_check("fmt=cargo fmt"))

    def test_name_is_stripped_but_args_are_not_merged(self):
        self.assertEqual(cloud_card.parse_check("  tests  =cargo   test  "),
                         {"name": "tests", "command": "cargo", "args": ["test"]})

    def test_rejects_specs_that_cannot_become_a_command(self):
        for spec in ("nosign", "=cargo test", "tests=", "tests=   ", ""):
            with self.subTest(spec=spec), self.assertRaises(argparse.ArgumentTypeError):
                cloud_card.parse_check(spec)

    def test_check_json_round_trips_fields_the_shorthand_cannot_express(self):
        raw = '{"name":"t","command":"python3","args":["-c","print(1) ; print(2)"],"cwd":"sub","expect_exit":3}'
        self.assertEqual(cloud_card.parse_check_json(raw)["args"][1], "print(1) ; print(2)")
        self.assertEqual(cloud_card.parse_check_json(raw)["expect_exit"], 3)

    def test_check_json_rejects_non_objects_and_bad_json(self):
        for raw in ('[{"name":"t"}]', '"t"', '{', 'null'):
            with self.subTest(raw=raw), self.assertRaises(argparse.ArgumentTypeError):
                cloud_card.parse_check_json(raw)


class TestAcceptanceReceipt(unittest.TestCase):
    """Finding the receipt in a card report.

    complete_card/fail_card persist report TEXT, not structured data, so the host appends
    `Acceptance checks: {json}` to the report (tools.rs:357). Parsing that line back is the only way
    a node-key caller sees what the checks did -- which makes this parser the whole read path for
    the gate's evidence."""

    def test_finds_the_receipt_after_the_model_prose(self):
        report = "I wrote the module and ran the tests.\n\nAcceptance checks: " \
                 '{"status":"passed","results":[{"name":"tests","passed":true}]}'
        self.assertEqual(cloud_card.receipt(report)["status"], "passed")

    def test_reads_a_failure_receipt_including_the_tails(self):
        report = 'done\nAcceptance checks: {"status":"failed","results":[' \
                 '{"name":"tests","passed":false,"exit_status":101,"stderr_tail":"test failed"}]}'
        got = cloud_card.receipt(report)
        self.assertEqual(got["status"], "failed")
        self.assertEqual(got["results"][0]["exit_status"], 101)

    def test_no_receipt_is_none_not_an_exception(self):
        """A card from a node that predates the acceptance build has no receipt at all; that has to
        read as absent rather than crashing the harness."""
        self.assertIsNone(cloud_card.receipt("just the model talking about itself"))
        self.assertIsNone(cloud_card.receipt(""))

    def test_malformed_receipt_is_none_rather_than_a_crash(self):
        self.assertIsNone(cloud_card.receipt("Acceptance checks: {not json"))

    def test_unverified_is_a_real_status_and_not_an_absent_receipt(self):
        """The distinction that matters: a card with no checks reports `unverified` and fails
        nothing, which is exactly the state the harness was stuck in before it could submit
        checks. It must not be confused with a missing receipt."""
        self.assertEqual(cloud_card.receipt('x\nAcceptance checks: {"status":"unverified"}'),
                         {"status": "unverified"})


if __name__ == '__main__':
    unittest.main()
