import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import telemetry_parser


SAMPLE = """GYROFLOW IMU LOG
version,1.3
id,Python_compatibility
vendor,Test
orientation,XYZ
tscale,0.001
gscale,0.017453292519943295
ascale,1
t,gx,gy,gz,ax,ay,az
0,1,2,3,0,0,1
10,4,5,6,1,0,0
"""


class ParserTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.path = Path(directory.name) / "sample.gcsv"
        self.path.write_text(SAMPLE)

    def test_telemetry_and_normalized_imu(self):
        parser = telemetry_parser.Parser(str(self.path))
        self.assertEqual(parser.camera, "Test")
        self.assertEqual(parser.model, "Python compatibility")

        telemetry = parser.telemetry()
        self.assertEqual(telemetry, parser.telemetry(None))
        self.assertEqual(telemetry, parser.telemetry(human_readable=False))
        self.assertEqual(
            telemetry[0]["Gyroscope"]["Data"],
            [
                {"t": 0.0, "x": 1.0, "y": 2.0, "z": 3.0},
                {"t": 0.01, "x": 4.0, "y": 5.0, "z": 6.0},
            ],
        )
        self.assertIsInstance(
            parser.telemetry(human_readable=True)[0]["Gyroscope"]["Data"], str
        )

        imu = parser.normalized_imu()
        self.assertEqual(imu, parser.normalized_imu(None))
        self.assertEqual(imu, parser.normalized_imu(orientation="XYZ"))
        self.assertEqual(
            imu,
            [
                {
                    "timestamp_ms": 0.0,
                    "gyro": (1.0, 2.0, 3.0),
                    "accl": (0.0, 0.0, 9.80665),
                    "magn": None,
                },
                {
                    "timestamp_ms": 10.0,
                    "gyro": (4.0, 5.0, 6.0),
                    "accl": (9.80665, 0.0, 0.0),
                    "magn": None,
                },
            ],
        )

    def test_parser_can_be_used_from_other_threads(self):
        parser = telemetry_parser.Parser(str(self.path))
        expected = (parser.telemetry(), parser.normalized_imu())
        with ThreadPoolExecutor(max_workers=2) as executor:
            results = list(
                executor.map(
                    lambda _: (parser.telemetry(), parser.normalized_imu()), range(4)
                )
            )
        self.assertEqual(results, [expected] * 4)


if __name__ == "__main__":
    unittest.main()
